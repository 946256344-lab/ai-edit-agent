# Offline scene-detection timing probe (optional)
# Usage: node scripts/probe-scene-detect.mjs <video1> [video2...]
# Records wall-clock ms and cut counts under the 60s budget logic used by segments.rs.

import { spawn } from 'node:child_process'
import { performance } from 'node:perf_hooks'

const budgetMs = 60_000
const videos = process.argv.slice(2)
if (videos.length === 0) {
  console.error('Usage: node scripts/probe-scene-detect.mjs <video...>')
  process.exit(1)
}

function run(video) {
  return new Promise((resolve) => {
    const started = performance.now()
    const args = [
      '-hide_banner',
      '-loglevel',
      'info',
      '-i',
      video,
      '-vf',
      "fps=3,scale=160:-2,select='gt(scene\\,0.30)',showinfo",
      '-an',
      '-f',
      'null',
      '-',
    ]
    const child = spawn('ffmpeg', args, { windowsHide: true })
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill()
    }, budgetMs)
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      const elapsed = Math.round(performance.now() - started)
      const cuts = [...stderr.matchAll(/pts_time:([0-9.]+)/g)].length
      resolve({ video, elapsedMs: elapsed, cuts, code, timedOut: elapsed >= budgetMs - 50 })
    })
  })
}

const rows = []
for (const video of videos) {
  rows.push(await run(video))
}
console.table(rows)
