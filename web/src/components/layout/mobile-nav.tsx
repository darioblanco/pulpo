import { Waves, LayoutList, History, Clock, Settings } from 'lucide-react';
import { Link, useLocation } from 'react-router';

const navItems = [
  { to: '/', icon: Waves, label: 'Ocean' },
  { to: '/sessions', icon: LayoutList, label: 'Sessions' },
  { to: '/history', icon: History, label: 'History' },
  { to: '/schedules', icon: Clock, label: 'Schedules' },
  { to: '/settings', icon: Settings, label: 'Settings' },
];

export function MobileNav() {
  const location = useLocation();

  return (
    <nav
      data-testid="mobile-nav"
      className="fixed bottom-0 left-0 right-0 z-50 flex border-t border-border bg-sidebar md:hidden"
      style={{ paddingBottom: 'env(safe-area-inset-bottom, 0px)' }}
    >
      {navItems.map((item) => {
        const isActive =
          item.to === '/' ? location.pathname === '/' : location.pathname.startsWith(item.to);
        return (
          <Link
            key={item.to}
            to={item.to}
            className={`flex flex-1 flex-col items-center gap-0.5 py-2 text-[0.6rem] ${
              isActive ? 'text-primary' : 'text-muted-foreground'
            }`}
          >
            <item.icon className="h-5 w-5" />
            {item.label}
          </Link>
        );
      })}
    </nav>
  );
}
