import { useEffect, useRef, useState } from 'react'
import hljs from 'highlight.js/lib/core'
import python from 'highlight.js/lib/languages/python'
import 'highlight.js/styles/github-dark-dimmed.css'

hljs.registerLanguage('python', python)

export function Code({ code, label, lang = 'python' }) {
  const ref = useRef(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (ref.current && !ref.current.dataset.highlighted) {
      hljs.highlightElement(ref.current)
    }
  }, [code])

  const copy = () => {
    navigator.clipboard.writeText(code.trim())
    setCopied(true)
    setTimeout(() => setCopied(false), 1800)
  }

  return (
    <div className="code-block">
      {label && (
        <div className="code-label">
          <span className="code-label-dot" />
          {label}
          <button className={`copy-btn ${copied ? 'copied' : ''}`} onClick={copy} style={{ marginLeft: 'auto' }}>
            {copied ? 'copied' : 'copy'}
          </button>
        </div>
      )}
      {!label && (
        <button
          className={`copy-btn ${copied ? 'copied' : ''}`}
          onClick={copy}
          style={{ position: 'absolute', top: 10, right: 12, zIndex: 1 }}
        >
          {copied ? 'copied' : 'copy'}
        </button>
      )}
      <pre>
        <code ref={ref} className={`language-${lang}`}>{code.trim()}</code>
      </pre>
    </div>
  )
}

export function Tabs({ tabs }) {
  const [active, setActive] = useState(0)
  return (
    <div className="tabs">
      <div className="tab-list">
        {tabs.map((t, i) => (
          <button key={t.label} className={`tab-btn ${i === active ? 'active' : ''}`} onClick={() => setActive(i)}>
            {t.label}
          </button>
        ))}
      </div>
      <Code code={tabs[active].code} label={null} lang={tabs[active].lang || 'python'} />
    </div>
  )
}
