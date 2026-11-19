import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './styles/index.css';
import { App } from './app/App';
import { ws } from '@shared/api/websocket';

// Start the WebSocket connection for the whole app lifetime
ws.connect();

const rootEl = document.getElementById('root');
if (!rootEl) throw new Error('Root element #root not found in DOM');

createRoot(rootEl).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
