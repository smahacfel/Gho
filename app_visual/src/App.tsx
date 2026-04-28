import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import AppShell from './components/layout/AppShell';
import Dashboard from './pages/Dashboard';
import Settings from './pages/Settings';
import Shadow from './pages/Shadow';

export default function App() {
  return (
    <BrowserRouter>
      {/* AppShell wraps all pages with the top bar and arrow navigation */}
      <AppShell>
        <Routes>
          <Route path="/"          element={<Navigate to="/dashboard" replace />} />
          <Route path="/dashboard" element={<Dashboard />} />
          <Route path="/settings"  element={<Settings />} />
          <Route path="/shadow"    element={<Shadow />} />
        </Routes>
      </AppShell>
    </BrowserRouter>
  );
}
