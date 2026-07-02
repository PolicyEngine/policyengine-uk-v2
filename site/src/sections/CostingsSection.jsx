import { useEffect, useMemo, useState } from 'react'

const fmtM = (v) => {
  if (v == null) return '—'
  const s = Math.round(v).toLocaleString('en-GB')
  return v > 0 ? `+${s}` : s
}

// The headline series: post-behavioural if the measure reports the expanded
// format, otherwise the plain Exchequer impact row, otherwise the first row.
function headlineRow(impacts) {
  if (!impacts.length) return null
  return (
    impacts.find((i) => /^post-behavioural exchequer impact/i.test(i.label)) ||
    impacts.find((i) => /^exchequer impact/i.test(i.label)) ||
    impacts[0]
  )
}

const valueClass = (n) => (n == null ? '' : n < 0 ? 'cal-bad' : 'cal-good')

function ImpactTable({ impacts }) {
  const years = useMemo(() => {
    const s = new Set()
    for (const i of impacts) for (const y of Object.keys(i.values)) s.add(y)
    return [...s].sort()
  }, [impacts])
  if (!impacts.length) return <p className="cost-none">No impact table in source document.</p>
  return (
    <div className="cal-table-wrap cost-impact-wrap">
      <table className="cal-table">
        <thead>
          <tr>
            <th className="cal-th">£m</th>
            {years.map((y) => <th key={y} className="cal-th">{y}</th>)}
          </tr>
        </thead>
        <tbody>
          {impacts.map((i, k) => (
            <tr key={k}>
              <td className="cal-name" title={i.label}>{i.label}</td>
              {years.map((y) => {
                const v = i.values[y]
                if (!v) return <td key={y}>—</td>
                return <td key={y} className={`cal-err ${valueClass(v.num)}`}>{v.num != null ? fmtM(v.num) : v.raw}</td>
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function TextBlock({ label, text }) {
  if (!text) return null
  return (
    <div className="cost-text">
      <div className="cost-text-label">{label}</div>
      <p>{text}</p>
    </div>
  )
}

const MODEL_LABEL = { yes: 'in model', partial: 'partial', no: 'not in model' }

function Detail({ m, ev }) {
  return (
    <div className="cost-detail">
      <ImpactTable impacts={m.impacts} />
      <TextBlock label="Measure description" text={m.description} />
      <TextBlock label="Tax / cost base" text={m.base} />
      <TextBlock label="Costing methodology" text={m.costing} />
      <TextBlock label="Impact table notes" text={m.impactNotes} />
      <TextBlock label="Areas of uncertainty" text={m.uncertainty} />
      <p className="cost-src">
        Source: <a href={ev.url} target="_blank" rel="noreferrer">{ev.name} policy costings</a>, p.{m.page}
      </p>
    </div>
  )
}

const eventYears = (measures) => {
  const s = new Set()
  for (const m of measures) {
    const row = headlineRow(m.impacts)
    if (row) for (const y of Object.keys(row.values)) s.add(y)
  }
  return [...s].sort()
}

export default function CostingsSection({ id }) {
  const [db, setDb] = useState(null)
  const [err, setErr] = useState(null)
  const [query, setQuery] = useState('')
  const [area, setArea] = useState('all')
  const [inModel, setInModel] = useState('all')
  const [incidence, setIncidence] = useState('all')
  const [openEvents, setOpenEvents] = useState({})
  const [openMeasure, setOpenMeasure] = useState(null)

  useEffect(() => {
    fetch(`${import.meta.env.BASE_URL}costings.json`)
      .then((r) => { if (!r.ok) throw new Error(`${r.status}`); return r.json() })
      .then(setDb)
      .catch((e) => setErr(e.message))
  }, [])

  const areas = useMemo(() => {
    if (!db) return []
    return [...new Set(db.flatMap((ev) => ev.measures.map((m) => m.area)))].sort()
  }, [db])

  const filtering = query.trim() !== '' || area !== 'all' || inModel !== 'all' || incidence !== 'all'

  const groups = useMemo(() => {
    if (!db) return []
    const q = query.trim().toLowerCase()
    const out = []
    for (const ev of db) {
      const measures = ev.measures.filter((m) => {
        if (area !== 'all' && m.area !== area) return false
        if (inModel !== 'all' && m.inModel !== inModel) return false
        if (incidence !== 'all' && m.incidence !== incidence) return false
        if (q && !`${m.title} ${m.description}`.toLowerCase().includes(q)) return false
        return true
      })
      if (measures.length) out.push({ ev, measures, years: eventYears(measures) })
    }
    return out
  }, [db, query, area, inModel, incidence])

  const nShown = groups.reduce((n, g) => n + g.measures.length, 0)
  const maxYears = Math.max(1, ...groups.map((g) => g.years.length))
  const nCols = 4 + maxYears

  return (
    <section className="section" id={id}>
      <div className="section-tag">08 — Policy costings</div>
      <h1>Policy costings database</h1>
      <p>
        Every measure from the HM Treasury policy costings documents published alongside
        each fiscal event from Budget 2016 to Budget 2025 — the exchequer impact (£m)
        over the forecast horizon as certified by the OBR, with the full measure
        description, tax/cost base, costing methodology and areas of uncertainty.
        Each measure is labelled, manually, for whether the lever it changes is
        represented in this model: <em>in model</em> means the specific parameter
        exists (e.g. the UC taper rate), <em>partial</em> means the instrument is
        modelled but the lever or population is not separately representable, and{' '}
        <em>not in model</em> means the instrument is out of scope (e.g. corporation
        tax). Incidence is statutory — who directly pays or receives. Negative values
        are exchequer costs, positive are yield; <code>neg</code> and <code>*</code>{' '}
        mean negligible. The same data ships as an xlsx workbook at{' '}
        <code>data/costings/uk_policy_costings_2016_2025.xlsx</code>.
      </p>

      {err && <div className="callout warn"><p>Could not load costings data ({err}). Run <code>data/costings/build_xlsx.py</code> to regenerate.</p></div>}
      {!db && !err && <p style={{ color: 'var(--text3)' }}>Loading costings database…</p>}

      {db && (
        <>
          <div className="cal-controls">
            <label>Search
              <input type="text" value={query} placeholder="title or description…" onChange={(e) => { setQuery(e.target.value); setOpenMeasure(null) }} />
            </label>
            <label>Policy area
              <select value={area} onChange={(e) => { setArea(e.target.value); setOpenMeasure(null) }}>
                <option value="all">all areas</option>
                {areas.map((a) => <option key={a} value={a}>{a}</option>)}
              </select>
            </label>
            <label>Model coverage
              <select value={inModel} onChange={(e) => { setInModel(e.target.value); setOpenMeasure(null) }}>
                <option value="all">all</option>
                <option value="yes">in model</option>
                <option value="partial">partial</option>
                <option value="no">not in model</option>
              </select>
            </label>
            <label>Incidence
              <select value={incidence} onChange={(e) => { setIncidence(e.target.value); setOpenMeasure(null) }}>
                <option value="all">all</option>
                <option value="households">households</option>
                <option value="firms">firms</option>
                <option value="mixed">mixed</option>
                <option value="public sector & other">public sector &amp; other</option>
              </select>
            </label>
          </div>

          <p className="cost-count">
            {nShown} measure{nShown === 1 ? '' : 's'} across {groups.length} event{groups.length === 1 ? '' : 's'} —
            click an event row to expand, a measure row for methodology detail. Rows are tinted by model coverage.
          </p>

          <div className="cal-table-wrap cost-table-wrap">
            <table className="cal-table cost-table">
              <thead>
                <tr>
                  <th className="cal-th">Measure</th>
                  <th className="cal-th">Policy area</th>
                  <th className="cal-th">Model</th>
                  <th className="cal-th">Incidence</th>
                  <th className="cal-th" colSpan={maxYears}>Exchequer impact (£m)</th>
                </tr>
              </thead>
              <tbody>
                {groups.map(({ ev, measures, years }) => {
                  const isOpen = filtering || !!openEvents[ev.key]
                  const nYes = measures.filter((m) => m.inModel === 'yes').length
                  const nPartial = measures.filter((m) => m.inModel === 'partial').length
                  const rows = [
                    <tr
                      key={ev.key}
                      className="cost-event-row"
                      onClick={() => setOpenEvents((o) => ({ ...o, [ev.key]: !o[ev.key] }))}
                    >
                      <td colSpan={nCols}>
                        <span className={`cost-chevron ${isOpen ? 'open' : ''}`}>▸</span>
                        <span className="cost-event-name">{ev.name}</span>
                        <span className="cost-event-meta">
                          {measures.length} measure{measures.length === 1 ? '' : 's'}
                          {' · '}<span className="cost-model-yes">{nYes} in model</span>
                          {nPartial > 0 && <>{' · '}<span className="cost-model-partial">{nPartial} partial</span></>}
                        </span>
                      </td>
                    </tr>,
                  ]
                  if (isOpen) {
                    rows.push(
                      <tr key={`${ev.key}-years`} className="cost-year-row">
                        <td colSpan={4}></td>
                        {years.map((y) => <td key={y}>{y}</td>)}
                        {Array.from({ length: maxYears - years.length }, (_, i) => <td key={`pad${i}`}></td>)}
                      </tr>
                    )
                    for (const m of measures) {
                      const key = `${ev.key}|${m.title}|${m.page}`
                      const row = headlineRow(m.impacts)
                      const isMOpen = openMeasure === key
                      rows.push(
                        <tr key={key} className={`cost-row cost-tint-${m.inModel}`} onClick={() => setOpenMeasure(isMOpen ? null : key)}>
                          <td className="cost-title">{m.title}</td>
                          <td className="cal-src">{m.area}</td>
                          <td className={`cost-model-${m.inModel}`}>{MODEL_LABEL[m.inModel] || m.inModel}</td>
                          <td className="cal-src">{m.incidence}</td>
                          {years.map((y) => {
                            const v = row?.values[y]
                            if (!v) return <td key={y} className="cal-err">—</td>
                            return <td key={y} className={`cal-err ${valueClass(v.num)}`}>{v.num != null ? fmtM(v.num) : v.raw}</td>
                          })}
                          {Array.from({ length: maxYears - years.length }, (_, i) => <td key={`pad${i}`}></td>)}
                        </tr>
                      )
                      if (isMOpen) {
                        rows.push(
                          <tr key={`${key}-detail`}>
                            <td colSpan={nCols} className="cost-detail-cell">
                              <Detail m={m} ev={ev} />
                            </td>
                          </tr>
                        )
                      }
                    }
                  }
                  return rows
                })}
              </tbody>
            </table>
          </div>
        </>
      )}
    </section>
  )
}
