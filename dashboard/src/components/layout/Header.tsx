import { NavLink } from 'react-router-dom';
import type { StatusV2 } from '../../api/types';

interface HeaderProps {
  status: StatusV2 | null;
  connected: boolean;
}

const navItems = [
  { to: '/', label: 'Live' },
  { to: '/hourly', label: 'Hourly' },
  { to: '/daily', label: 'Daily' },
  { to: '/reports', label: 'Reports' },
  { to: '/feedback', label: 'Feedback' },
];

export function Header({ status, connected }: HeaderProps) {
  return (
    <header className="bg-bg-secondary border-b border-border px-4 py-2 flex items-center justify-between shrink-0">
      <div className="flex items-center gap-4">
        <h1 className="text-accent-blue font-bold text-sm tracking-wider">
          SAIREN-OS
        </h1>
        <nav className="flex gap-1">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.to === '/'}
              className={({ isActive }) =>
                `px-3 py-1 rounded text-xs font-medium transition-colors ${
                  isActive
                    ? 'bg-accent-blue/20 text-accent-blue'
                    : 'text-text-secondary hover:text-text-primary hover:bg-bg-hover'
                }`
              }
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
      </div>
      <div className="flex items-center gap-3 text-xs">
        {status && (
          <>
            <span className="text-text-secondary">{status.campaign}</span>
            <span className="text-text-muted">|</span>
            <span className="text-text-secondary">{status.operation}</span>
          </>
        )}
        <span
          className={`w-2 h-2 rounded-full ${
            connected ? 'bg-accent-green' : 'bg-accent-red animate-pulse'
          }`}
          title={connected ? 'Connected' : 'Disconnected'}
        />
      </div>
    </header>
  );
}
