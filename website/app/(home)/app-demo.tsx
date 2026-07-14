'use client';

import { useEffect, useRef, useState } from 'react';

/* An auto-playing mock of the Ogma desktop app. It loops through the three
   moments that define the product — recording, the processing pipeline, and
   the finished speaker-labeled notes — with no user interaction. The visuals
   mirror the real app UI (src/styles.css) so the landing shows the actual
   thing, not a stock screenshot. */

type Phase = 'record' | 'proc' | 'notes';

// Fixed bar heights (px) for the record equalizer — deterministic so server
// and client render identically; CSS animates each bar's scaleY.
const BARS = [
  16, 30, 44, 22, 38, 52, 28, 46, 18, 34, 50, 24, 40, 30, 48, 20, 36, 52, 26,
  42, 32, 50, 22, 38, 28, 46, 18, 34, 44, 24, 40, 30, 48, 20,
];

const STEPS = [
  { name: 'Transcribe', sub: 'OpenAI Whisper · 5-min chunks' },
  { name: 'Speakers + notes', sub: 'Claude · one structured call' },
  { name: 'Push to Notion', sub: 'page + transcript toggle block' },
];

const BASE_SECONDS = 41 * 60 + 12; // an ongoing, hour-ish meeting

function clock(total: number): string {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

export function AppDemo() {
  const [phase, setPhase] = useState<Phase>('record');
  const [procStep, setProcStep] = useState(0);
  const [seconds, setSeconds] = useState(BASE_SECONDS);
  const phaseRef = useRef<Phase>('record');
  phaseRef.current = phase;

  // Phase scheduler: record → processing (stepped) → notes → loop.
  useEffect(() => {
    const timers: ReturnType<typeof setTimeout>[] = [];
    let cancelled = false;
    const at = (fn: () => void, ms: number) => {
      timers.push(setTimeout(() => !cancelled && fn(), ms));
    };

    const cycle = () => {
      setPhase('record');
      setSeconds(BASE_SECONDS);
      at(() => {
        setPhase('proc');
        setProcStep(0);
        at(() => setProcStep(1), 1500);
        at(() => setProcStep(2), 3000);
        at(() => setProcStep(3), 4500);
        at(() => {
          setPhase('notes');
          at(cycle, 7000);
        }, 5400);
      }, 6500);
    };
    cycle();

    return () => {
      cancelled = true;
      timers.forEach(clearTimeout);
    };
  }, []);

  // The recording clock only advances while the record scene is on screen.
  useEffect(() => {
    const id = setInterval(() => {
      if (phaseRef.current === 'record') setSeconds((s) => s + 1);
    }, 1000);
    return () => clearInterval(id);
  }, []);

  const activeNav = phase === 'record' ? 'record' : 'library';

  return (
    <div className="demo" role="img" aria-label="Animated preview of the Ogma desktop app recording, processing, and producing meeting notes">
      {/* title bar */}
      <div className="demo-titlebar">
        <div className="demo-lights">
          <span />
          <span />
          <span />
        </div>
        <span className="demo-tb-label">OGMA — MEETING RECORDER</span>
        <span className="lp-spacer" />
        <span className="demo-tb-toggle">◑ DARK</span>
      </div>

      <div className="demo-body">
        {/* sidebar */}
        <aside className="demo-sidebar">
          <div className="demo-brand">
            <img src="/ogma-logo.png" alt="" />
            <span>ogma</span>
          </div>
          <nav className="demo-nav">
            <div className={`demo-nav-item${activeNav === 'record' ? ' active' : ''}`}>
              Record
              {phase === 'record' && (
                <span className="demo-nav-badge">
                  <span className="dot" />
                  REC
                </span>
              )}
            </div>
            <div className={`demo-nav-item${activeNav === 'library' ? ' active' : ''}`}>
              Library
            </div>
            <div className="demo-nav-item">Settings</div>
          </nav>
          <div className="demo-status">
            <div className="demo-status-line">
              <span className="demo-status-dot ok" />
              MCP server · stdio
            </div>
            <div className="demo-status-line">
              <span className={`demo-status-dot ${phase === 'notes' ? 'ok' : 'muted'}`} />
              {phase === 'notes' ? 'Notion · synced' : 'Notion · connected'}
            </div>
          </div>
        </aside>

        {/* main stage */}
        <div className="demo-main">
          {/* RECORD */}
          <div className={`demo-scene demo-scene-record${phase === 'record' ? ' show' : ''}`}>
            <div className="demo-rec-title">Q3 Planning Sync</div>
            <div className="demo-rec-wave" aria-hidden>
              {BARS.map((hgt, i) => (
                <span
                  key={i}
                  style={{
                    height: `${hgt}px`,
                    animationDuration: `${1.0 + (i % 6) * 0.16}s`,
                    animationDelay: `${(i % 9) * 0.08}s`,
                  }}
                />
              ))}
            </div>
            <div className="demo-rec-timer">{clock(seconds)}</div>
            <button className="demo-rec-btn" aria-hidden tabIndex={-1}>
              <span />
            </button>
            <div className="demo-rec-status">
              recording — seg-008.wav
              <br />
              crash-safe · rotating 5-min WAV segments · 16 kHz mono
            </div>
          </div>

          {/* PROCESSING */}
          <div className={`demo-scene demo-scene-proc${phase === 'proc' ? ' show' : ''}`}>
            <div className="demo-proc-head">Processing “Q3 Planning Sync”</div>
            {STEPS.map((step, i) => {
              const state =
                i < procStep ? 'done' : i === procStep ? 'active' : 'waiting';
              return (
                <div key={step.name} className={`demo-step ${state}`}>
                  <div className="demo-step-mark">
                    {state === 'done' ? (
                      '✓'
                    ) : state === 'active' ? (
                      <span className="spin" />
                    ) : (
                      i + 1
                    )}
                  </div>
                  <div className="demo-step-text">
                    <span className="demo-step-name">{step.name}</span>
                    <span className="demo-step-sub">{step.sub}</span>
                  </div>
                </div>
              );
            })}
          </div>

          {/* NOTES */}
          <div className={`demo-scene demo-scene-notes${phase === 'notes' ? ' show' : ''}`}>
            <div className="demo-notes-head">
              <span className="demo-notes-title">Q3 Planning Sync</span>
              <span className="demo-synced">SYNCED ✓</span>
            </div>
            <div className="demo-panel">
              <div className="demo-panel-label">TL;DR</div>
              <div className="demo-tldr">
                Team aligned on shipping the mobile beta this quarter — Maya owns
                the launch checklist; Dan clears the API rate-limit work first.
              </div>
            </div>
            <div className="demo-panel">
              <div className="demo-panel-label">ACTION ITEMS</div>
              <div className="demo-action">
                <span className="demo-check">✓</span>
                Maya — finalize the launch checklist by Friday
              </div>
              <div className="demo-action">
                <span className="demo-check">✓</span>
                Dan — raise the API rate limit before the beta
              </div>
            </div>
            <div className="demo-panel">
              <div className="demo-panel-label">TRANSCRIPT</div>
              <div className="demo-turn">
                <span
                  className="demo-chip"
                  style={{
                    color: 'var(--ac)',
                    background: 'color-mix(in oklab, var(--ac) 12%, transparent)',
                    border: '1px solid color-mix(in oklab, var(--ac) 30%, transparent)',
                  }}
                >
                  Maya
                </span>
                <span className="demo-turn-text">
                  Let’s lock the beta date so marketing can plan around it.
                </span>
              </div>
              <div className="demo-turn">
                <span
                  className="demo-chip"
                  style={{
                    color: 'var(--vi)',
                    background: 'color-mix(in oklab, var(--vi) 12%, transparent)',
                    border: '1px solid color-mix(in oklab, var(--vi) 30%, transparent)',
                  }}
                >
                  Dan
                </span>
                <span className="demo-turn-text">
                  Works for me — I’ll clear the rate-limit ticket this week.
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
