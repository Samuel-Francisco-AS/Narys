import React from 'react'
import ReactDOM from 'react-dom/client'
import './styles.css'

const surface = new URLSearchParams(window.location.search).get('surface')
const load = surface === 'settings-general'
  ? import('./settings/GeneralSettingsApp')
  : surface === 'settings-ai'
    ? import('./settings/AiSettingsApp')
    : import('./App')
void load.then(({ default: Surface }) => {
  ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode><Surface /></React.StrictMode>)
})
