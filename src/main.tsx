import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'

// Renderer entry only. App owns desktop bootstrap and composes the domain
// controllers; filesystem, database, model, and media side effects stay behind
// named Tauri commands in the Rust crate.
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
