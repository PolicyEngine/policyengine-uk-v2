import { useState, useEffect, useRef } from 'react'
import GettingStarted from './sections/GettingStarted'
import SimulationSection from './sections/SimulationSection'
import ParametersSection from './sections/ParametersSection'
import ResultsSection from './sections/ResultsSection'
import StructuralSection from './sections/StructuralSection'
import DatasetsSection from './sections/DatasetsSection'

const NAV = [
  { id: 'getting-started', label: 'Getting started' },
  { id: 'simulation', label: <><code>Simulation</code></> },
  { id: 'parameters', label: <><code>Parameters</code></> },
  { id: 'results', label: 'Results' },
  { id: 'structural', label: 'Structural reforms' },
  { id: 'datasets', label: 'Datasets' },
]

export default function App() {
  const [theme, setTheme] = useState('light')
  const [active, setActive] = useState('getting-started')
  const observerRef = useRef(null)

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])

  useEffect(() => {
    const sections = NAV.map(n => document.getElementById(n.id)).filter(Boolean)
    observerRef.current = new IntersectionObserver(
      entries => {
        const visible = entries.filter(e => e.isIntersecting)
        if (visible.length > 0) {
          // Pick the one closest to top
          const top = visible.sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0]
          setActive(top.target.id)
        }
      },
      { rootMargin: '-20% 0px -60% 0px', threshold: 0 }
    )
    sections.forEach(s => observerRef.current.observe(s))
    return () => observerRef.current?.disconnect()
  }, [])

  const scrollTo = (id) => {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="sidebar-header">
          <div className="sidebar-logo">
            policyengine-uk-compiled
            <span>Python API reference</span>
          </div>
        </div>
        <nav className="sidebar-nav">
          {NAV.map(item => (
            <button
              key={item.id}
              className={`nav-item ${active === item.id ? 'active' : ''}`}
              onClick={() => scrollTo(item.id)}
            >
              {item.label}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <span className="version-badge">v0.41.0</span>
          <button
            className="theme-toggle"
            onClick={() => setTheme(t => t === 'dark' ? 'light' : 'dark')}
            title="Toggle theme"
          >
            {theme === 'dark' ? '○' : '●'}
          </button>
        </div>
      </aside>

      <main className="content">
        <GettingStarted id="getting-started" />
        <hr className="divider" />
        <SimulationSection id="simulation" />
        <hr className="divider" />
        <ParametersSection id="parameters" />
        <hr className="divider" />
        <ResultsSection id="results" />
        <hr className="divider" />
        <StructuralSection id="structural" />
        <hr className="divider" />
        <DatasetsSection id="datasets" />
      </main>
    </div>
  )
}
