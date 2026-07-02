"""Extract per-measure exchequer impact tables from HMT policy costings PDFs."""

import json
import re
from pathlib import Path

import pdfplumber

HERE = Path(__file__).parent
YEAR_RE = re.compile(r"^20(\d\d)[-/](\d\d)$")

BASE_HEADS = {
    "the tax base", "the cost base", "the cost/tax base", "the tax/cost base",
    "tax base", "cost base", "the base", "the costing base",
    "tax base and data", "cost base and data", "the tax base and data",
}
COSTING_HEADS = {"costing", "the costing", "static costing", "post-behavioural costing", "behavioural response"}
UNCERTAINTY_HEADS = {"areas of uncertainty", "areas of additional uncertainty"}
IMPACT_HEAD_RE = re.compile(
    r"^(static |post-behavioural )?exchequer impact(s)? \(£ ?m(illion)?\)$", re.I
)
VALUE_TOKEN_RE = re.compile(r"^[+\-–]?[\d,]+m?$|^neg$|^\*$|^0m?$", re.I)


def clean_cell(c):
    return " ".join((c or "").split())


def norm_year(c):
    """Return normalised fiscal year '20XX-YY' if cell is a year, else None."""
    s = re.sub(r"\s", "", c or "")
    m = YEAR_RE.match(s)
    return f"20{m.group(1)}-{m.group(2)}" if m else None


def is_year_header(row):
    return sum(1 for c in row if norm_year(c)) >= 3


def parse_pdf(path):
    """Return list of measures: title, pages, description, uncertainty, tables."""
    measures = []
    with pdfplumber.open(path) as pdf:
        pages = []
        for p in pdf.pages:
            text = p.extract_text() or ""
            pages.append({"num": p.page_number, "text": text, "tables": p.extract_tables()})

    # find annex start to exclude indexation tables
    annex_start = None
    for pg in pages:
        for l in pg["text"].split("\n")[:3]:
            # contents entries end with a page number; the real heading doesn't
            if re.match(r"^(Annex A|A Indexation)", l.strip()) and not re.search(r"\d+$", l.strip()):
                annex_start = pg["num"]
                break
        if annex_start:
            break

    current = None
    for pg in pages:
        if annex_start and pg["num"] >= annex_start:
            break
        lines = pg["text"].split("\n")
        md_idx = next(
            (i for i, l in enumerate(lines) if l.strip() == "Measure description"),
            None,
        )
        if md_idx is not None and md_idx <= 6:
            # new measure starts on this page
            if current:
                measures.append(current)
            title = " ".join(l.strip() for l in lines[:md_idx] if l.strip())
            title = re.sub(r"^\d+\s+", "", title)
            current = {
                "title": title,
                "page_start": pg["num"],
                "text": pg["text"],
                "tables": [],
            }
        elif current:
            current["text"] += "\n" + pg["text"]
        else:
            continue
        current["tables"].extend(pg["tables"] or [])
    if current:
        measures.append(current)

    for m in measures:
        m.update(extract_sections(m["text"]))
        m["impacts"] = parse_tables(m["tables"])
        del m["text"], m["tables"]
    return measures


def extract_sections(text):
    """Split a measure's text into description / base / costing / impact-notes / uncertainty."""
    sections = {"description": [], "base": [], "costing": [], "impact_notes": [], "uncertainty": []}
    current = None
    for l in text.split("\n"):
        s = l.strip()
        low = s.lower()
        if s == "Measure description":
            current = "description"
            continue
        if low in BASE_HEADS:
            current = "base"
            continue
        if low in COSTING_HEADS:
            current = "costing"
            # keep expanded-format subheadings inline
            if low not in ("costing", "the costing"):
                sections["costing"].append(f"[{s}]")
            continue
        if IMPACT_HEAD_RE.match(low):
            current = "impact_notes"
            continue
        if low in UNCERTAINTY_HEADS:
            current = "uncertainty"
            continue
        if current is None or not s:
            continue
        if re.fullmatch(r"\d+", s):  # page number
            continue
        if current == "impact_notes" and not is_prose_line(s):
            continue
        sections[current].append(s)
    out = {}
    for k, lines in sections.items():
        out[k] = re.sub(r"\s+", " ", " ".join(lines)).strip()
    return out


def is_prose_line(s):
    """Filter table fragments out of text under an 'Exchequer impact' heading."""
    tokens = s.split()
    if sum(1 for t in tokens if VALUE_TOKEN_RE.match(t) or norm_year(t)) >= 2:
        return False
    if len(tokens) <= 4 and not re.search(r"[.:;]$", s):
        return False
    return True


def parse_tables(tables):
    """Return list of {label, values: {year: raw}}.

    Tables can continue onto the next page without repeating the year
    header, so carry the last seen header forward when widths match.
    """
    rows_out = []
    years = None
    for t in tables:
        header = next((r for r in t[:2] if is_year_header(r)), None)
        if header:
            years = [norm_year(c) for c in header]
        elif not years or not t or len(t[0]) != len(years):
            continue
        pending_label = None
        for row in t:
            if header is not None and row is header:
                continue
            label = clean_cell(row[0])
            if label.lower() == "year" or is_year_header(row):
                continue
            vals = {}
            for y, c in zip(years, row):
                if y:
                    vals[y] = clean_cell(c)
            has_vals = any(v for v in vals.values())
            if label and not has_vals:
                # label row split from its values row (pdf line wrapping)
                pending_label = label
                continue
            if not label:
                if not (pending_label and has_vals):
                    continue
                label = pending_label
            pending_label = None
            if has_vals:
                rows_out.append({"label": label, "values": vals})
    return rows_out


def main():
    events = []
    for line in (HERE / "events.tsv").read_text().strip().split("\n"):
        key, name, date, url = line.split("\t")
        events.append({"key": key, "name": name, "date": date, "url": url})

    db = []
    for ev in events:
        path = HERE / "pdfs" / f"{ev['key']}.pdf"
        measures = parse_pdf(path)
        n_with = sum(1 for m in measures if m["impacts"])
        print(f"{ev['key']}: {len(measures)} measures, {n_with} with impact tables")
        db.append({**ev, "measures": measures})

    (HERE / "extracted.json").write_text(json.dumps(db, indent=1))


if __name__ == "__main__":
    main()
