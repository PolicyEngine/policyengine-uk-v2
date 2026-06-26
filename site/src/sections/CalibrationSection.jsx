import { useEffect, useMemo, useRef, useState } from 'react'
import * as d3 from 'd3'

// SPI band targets are named {prefix}_{threshold}_{year}; the threshold uprates
// with earnings every year, so the same conceptual band gets a different number
// each year. Group by prefix within a year and rank thresholds top-down so the
// persistent high-income bands stay aligned across years (bands are occasionally
// inserted mid-series, so ranking from the top is the stable key).
const SPI_RE = /^(spi\d*_[a-z_]+?)_(\d+)_(\d{4})$/
const stemOf = (name) => name.replace(/_\d{4}$/, '')

function alignKeys(targets) {
  const byPrefix = new Map()
  for (const t of targets) {
    const m = SPI_RE.exec(t.name)
    if (m) {
      if (!byPrefix.has(m[1])) byPrefix.set(m[1], [])
      byPrefix.get(m[1]).push({ name: t.name, thr: +m[2] })
    }
  }
  const out = new Map()
  byPrefix.forEach((arr, pfx) => {
    arr.sort((a, b) => b.thr - a.thr)
    const n = arr.length
    arr.forEach((e, i) => {
      out.set(e.name, { key: `${pfx} #t${String(i + 1).padStart(2, '0')}`, label: `${pfx} ·t${i + 1}/${n}` })
    })
  })
  for (const t of targets) {
    if (!out.has(t.name)) { const s = stemOf(t.name); out.set(t.name, { key: s, label: s }) }
  }
  return out
}

const fmtMoney = (v) => {
  if (v == null) return '—'
  const a = Math.abs(v)
  if (a >= 1e9) return '£' + (v / 1e9).toFixed(1) + 'bn'
  if (a >= 1e6) return '£' + (v / 1e6).toFixed(1) + 'm'
  if (a >= 1e3) return (v / 1e3).toFixed(0) + 'k'
  return v.toFixed(0)
}
// Headcount targets (people/benunits/households/claimants) are counts, not £.
const fmtCount = (v) => {
  if (v == null) return '—'
  const a = Math.abs(v)
  if (a >= 1e6) return (v / 1e6).toFixed(2) + 'm'
  if (a >= 1e3) return (v / 1e3).toFixed(0) + 'k'
  return v.toFixed(0)
}
const fmtValue = (v, unit) => (unit === 'count' ? fmtCount(v) : fmtMoney(v))
const fmtPct = (v) => v == null ? '—' : (v * 100 >= 0 ? '+' : '') + (v * 100).toFixed(1) + '%'
const errClass = (v) => { if (v == null) return ''; const a = Math.abs(v); return a < 0.05 ? 'cal-good' : a < 0.15 ? 'cal-warn' : 'cal-bad' }

function Heatmap({ data, src, trained, query }) {
  const ref = useRef(null)
  const tipRef = useRef(null)
  const containerRef = useRef(null)

  const { years, rows, maxErr } = useMemo(() => {
    const years = data.summary.map((s) => s.year)
    const stems = new Map() // key -> { label, errs: {year: pct} }
    for (const yr of years) {
      const keys = alignKeys(data.years[yr].targets)
      for (const t of data.years[yr].targets) {
        if (src !== 'all' && t.source !== src) continue
        if (query && !t.name.toLowerCase().includes(query)) continue
        if (trained === 'trained' && !t.trained) continue
        if (trained === 'untrained' && t.trained) continue
        if (t.rel_err_final == null) continue
        // Untrained targets aren't calibrated, so their error isn't a fit signal —
        // leave the cell blank in the combined view rather than colouring it red.
        // The 'untrained only' filter still shows them for drill-down.
        if (trained === 'all' && !t.trained) continue
        const { key, label } = keys.get(t.name)
        if (!stems.has(key)) stems.set(key, { label, errs: {} })
        stems.get(key).errs[yr] = t.rel_err_final * 100
      }
    }
    const rows = [...stems.values()].map((v) => {
      const vals = years.map((yr) => (yr in v.errs ? v.errs[yr] : null))
      const worst = Math.max(0, ...vals.filter((x) => x != null).map(Math.abs))
      return { label: v.label, vals, worst }
    }).sort((a, b) => b.worst - a.worst)
    const maxErr = Math.max(1, ...rows.map((r) => r.worst))
    return { years, rows, maxErr }
  }, [data, src, trained, query])

  useEffect(() => {
    const svg = d3.select(ref.current)
    svg.selectAll('*').remove()

    const cell = 13, labelW = 250, top = 24, right = 16
    const w = labelW + years.length * cell + right
    const h = top + rows.length * cell + 4
    svg.attr('width', w).attr('height', h).attr('viewBox', `0 0 ${w} ${h}`)

    const x = d3.scaleBand().domain(years).range([labelW, labelW + years.length * cell])
    const y = d3.scaleBand().domain(d3.range(rows.length)).range([top, top + rows.length * cell])

    // Continuous scale on |relative error %|. Most targets fit tightly but a
    // handful miss by several percent, so a sqrt transform keeps the small
    // differences visible rather than washing the grid green, while still
    // separating the worst misses. Domain caps at the larger of 1% or the
    // worst visible error, clamped, with green (good) → red (bad) via RdYlGn.
    const colour = d3.scaleSequentialSqrt(d3.interpolateRdYlGn).domain([maxErr, 0]).clamp(true)
    const cellColor = (v) => v == null ? 'transparent' : colour(Math.abs(v))

    // Year column headers.
    svg.append('g').selectAll('text').data(years).join('text')
      .attr('x', (d) => x(d) + cell / 2).attr('y', top - 8)
      .attr('text-anchor', 'middle').attr('class', 'cal-hm-col')
      .text((d) => `'${String(d).slice(2)}`)

    // Row labels.
    svg.append('g').selectAll('text').data(rows).join('text')
      .attr('x', labelW - 8).attr('y', (_, i) => y(i) + cell / 2 + 3)
      .attr('text-anchor', 'end').attr('class', 'cal-hm-row')
      .text((d) => d.label.length > 42 ? d.label.slice(0, 41) + '…' : d.label)
      .append('title').text((d) => d.label)

    const tip = d3.select(tipRef.current)
    const rowsG = svg.append('g')
    rows.forEach((r, i) => {
      rowsG.append('g').selectAll('rect')
        .data(r.vals.map((v, j) => ({ v, yr: years[j], label: r.label })))
        .join('rect')
        .attr('x', (d) => x(d.yr) + 0.5).attr('y', y(i) + 0.5)
        .attr('width', cell - 1).attr('height', cell - 1)
        .attr('fill', (d) => cellColor(d.v))
        .attr('stroke', (d) => d.v == null ? 'var(--border)' : 'none')
        .attr('stroke-width', 0.5)
        .style('cursor', (d) => d.v == null ? 'default' : 'pointer')
        .on('mousemove', (event, d) => {
          if (d.v == null) return
          // Position relative to the outer (non-scrolling) container. event.offset*
          // is relative to the hovered rect, which pins the tip to a cell corner;
          // clientX/Y minus the container box tracks the cursor instead.
          const box = containerRef.current.getBoundingClientRect()
          const tx = event.clientX - box.left
          const ty = event.clientY - box.top
          tip.style('opacity', 1)
            .style('left', tx + 14 + 'px')
            .style('top', ty + 'px')
            .html(`<strong>${d.label}</strong><br/>${d.yr}: ${d.v >= 0 ? '+' : ''}${d.v.toFixed(1)}%`)
        })
        .on('mouseleave', () => tip.style('opacity', 0))
    })
  }, [years, rows])

  // Legend: green (0%) left → red (maxErr) right, sqrt-spaced to match cells.
  // Visual position p maps to |err| = maxErr·p², so colour = RdYlGn(1-p).
  const legendStops = d3.range(0, 1.0001, 0.05).map((p) =>
    `${d3.interpolateRdYlGn(1 - p)} ${(p * 100).toFixed(0)}%`)
  const legendTicks = [0, maxErr * 0.25, maxErr] // |err| at left, mid, right of the sqrt bar
  return (
    <div style={{ position: 'relative' }} ref={containerRef}>
      <div className="cal-hm-legend">
        <span className="cal-hm-legend-label">|relative error|</span>
        <div className="cal-hm-legend-bar" style={{ background: `linear-gradient(to right, ${legendStops.join(', ')})` }} />
        <div className="cal-hm-legend-ticks">
          {legendTicks.map((v, i) => (
            <span key={i}>{v === 0 ? '0%' : v < 0.1 ? v.toFixed(2) + '%' : v.toFixed(1) + '%'}</span>
          ))}
        </div>
      </div>
      <div className="cal-hm-wrap">
        <svg ref={ref} />
      </div>
      <div ref={tipRef} className="cal-tip" />
    </div>
  )
}

function TargetTable({ data, year, src, trained, query }) {
  const [sortKey, setSortKey] = useState('improvement')
  const [sortAsc, setSortAsc] = useState(false)

  const rows = useMemo(() => {
    const yd = data.years[year]
    let r = yd.targets.filter((t) => {
      if (src !== 'all' && t.source !== src) return false
      if (query && !t.name.toLowerCase().includes(query)) return false
      if (trained === 'trained' && !t.trained) return false
      if (trained === 'untrained' && t.trained) return false
      return true
    }).map((t) => ({
      ...t,
      improvement: (t.rel_err_final == null || t.rel_err_initial == null)
        ? null : Math.abs(t.rel_err_final) - Math.abs(t.rel_err_initial),
    }))
    r.sort((a, b) => {
      let av = a[sortKey], bv = b[sortKey]
      if (sortKey === 'rel_err_initial' || sortKey === 'rel_err_final') { av = av == null ? -1 : Math.abs(av); bv = bv == null ? -1 : Math.abs(bv) }
      if (sortKey === 'improvement') { av = av == null ? 0 : av; bv = bv == null ? 0 : bv }
      if (typeof av === 'string') return sortAsc ? av.localeCompare(bv) : bv.localeCompare(av)
      return sortAsc ? av - bv : bv - av
    })
    return r
  }, [data, year, src, trained, query, sortKey, sortAsc])

  const sort = (k) => {
    if (sortKey === k) setSortAsc(!sortAsc)
    else { setSortKey(k); setSortAsc(k === 'name' || k === 'source') }
  }
  const th = (k, label) => (
    <th onClick={() => sort(k)} className={`cal-th ${sortKey === k ? 'sorted ' + (sortAsc ? 'asc' : 'desc') : ''}`}>{label}</th>
  )

  return (
    <div className="cal-table-wrap">
      <table className="cal-table">
        <thead><tr>
          {th('name', 'Target')}
          {th('source', 'Source')}
          {th('actual', 'Actual')}
          {th('pred_initial', 'Start pred')}
          {th('pred_final', 'Final pred')}
          {th('rel_err_initial', 'Start err')}
          {th('rel_err_final', 'Final err')}
          {th('improvement', '|Δ| err')}
          {th('trained', 'Trained')}
        </tr></thead>
        <tbody>
          {rows.map((t) => {
            const imp = t.improvement
            const impStr = imp == null ? '—' : (imp <= 0 ? '' : '+') + (imp * 100).toFixed(1) + 'pp'
            const impCls = imp == null ? '' : imp < -0.001 ? 'cal-good' : imp > 0.001 ? 'cal-bad' : ''
            return (
              <tr key={t.name}>
                <td className="cal-name" title={t.name}>{t.name}</td>
                <td className="cal-src">{t.source}</td>
                <td>{fmtValue(t.actual, t.unit)}</td>
                <td>{fmtValue(t.pred_initial, t.unit)}</td>
                <td>{fmtValue(t.pred_final, t.unit)}</td>
                <td className={`cal-err ${errClass(t.rel_err_initial)}`}>{fmtPct(t.rel_err_initial)}</td>
                <td className={`cal-err ${errClass(t.rel_err_final)}`}>{fmtPct(t.rel_err_final)}</td>
                <td className={`cal-err ${impCls}`}>{impStr}</td>
                <td><span className={`cal-pill ${t.trained ? '' : 'untrained'}`}>{t.trained ? 'yes' : 'no'}</span></td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

export default function CalibrationSection({ id }) {
  const [data, setData] = useState(null)
  const [err, setErr] = useState(null)
  const [src, setSrc] = useState('all')
  const [trained, setTrained] = useState('all')
  const [query, setQuery] = useState('')
  const [year, setYear] = useState(null)

  useEffect(() => {
    fetch(`${import.meta.env.BASE_URL}calibration.json`)
      .then((r) => { if (!r.ok) throw new Error(`${r.status}`); return r.json() })
      .then((d) => { setData(d); setYear(String(d.summary[d.summary.length - 1].year)) })
      .catch((e) => setErr(e.message))
  }, [])

  const sources = useMemo(() => {
    if (!data || !year) return []
    return Array.from(new Set(data.years[year].targets.map((t) => t.source))).sort()
  }, [data, year])

  const q = query.trim().toLowerCase()

  return (
    <section className="section" id={id}>
      <div className="section-tag">07 — Calibration</div>
      <h1>Calibration fit</h1>
      <p>
        Each EFRS year is reweighted so household-level aggregates match administrative
        targets (HMRC SPI income bands, DWP benefit expenditure and caseloads, the
        Stat-Xplore UC award-amount, element and in-work breakdowns, ONS consumption,
        OBR labour market). The heatmap shows the final relative error for
        every target in every year on a continuous green-to-red scale (sqrt-spaced so
        small differences stay visible) — green is a tight fit, red the worst
        miss in view. Forecast years
        (2025–2029) uprate the 2024 base and calibrate to projected targets.
      </p>

      {err && <div className="callout warn"><p>Could not load calibration data ({err}). Run <code>make report</code> to regenerate.</p></div>}
      {!data && !err && <p style={{ color: 'var(--text3)' }}>Loading calibration diagnostics…</p>}

      {data && year && (
        <>
          <div className="cal-controls">
            <label>Source
              <select value={src} onChange={(e) => setSrc(e.target.value)}>
                <option value="all">all sources</option>
                {sources.map((s) => <option key={s} value={s}>{s}</option>)}
              </select>
            </label>
            <label>Show
              <select value={trained} onChange={(e) => setTrained(e.target.value)}>
                <option value="all">all targets</option>
                <option value="trained">trained only</option>
                <option value="untrained">untrained only</option>
              </select>
            </label>
            <label>Filter
              <input type="text" value={query} placeholder="name contains…" onChange={(e) => setQuery(e.target.value)} />
            </label>
          </div>

          <h2>Fit heatmap — final relative error by target × year</h2>
          <Heatmap data={data} src={src} trained={trained} query={q} />

          <h2>
            Targets —
            <select className="cal-year-sel" value={year} onChange={(e) => setYear(e.target.value)}>
              {data.summary.map((s) => <option key={s.year} value={String(s.year)}>{s.year}</option>)}
            </select>
          </h2>
          <TargetTable data={data} year={year} src={src} trained={trained} query={q} />
        </>
      )}
    </section>
  )
}
