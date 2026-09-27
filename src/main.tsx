import React from 'react'
import ReactDOM from 'react-dom/client'
import './styles.css'

const auxiliary = new URLSearchParams(window.location.search).get('aux')
const root = ReactDOM.createRoot(document.getElementById('root')!)
if (auxiliary === 'composer' || auxiliary === 'conversation') {
  void import('./window/AuxiliaryPoc').then(({ AuxiliaryPoc }) => root.render(
    <React.StrictMode><AuxiliaryPoc surface={auxiliary} /></React.StrictMode>,
  ))
} else {
  void import('./App').then(({ default: App }) => root.render(
    <React.StrictMode><App /></React.StrictMode>,
  ))
}
