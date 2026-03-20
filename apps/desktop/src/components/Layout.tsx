import { NavLink, Outlet } from "react-router-dom";
import { Box, Store, UserCircle2, LifeBuoy } from "lucide-react";

export function Layout() {
  return (
    <div className="flex h-screen">
      {/* Sidebar */}
      <aside className="w-56 bg-white border-r border-neutral-200 flex flex-col shrink-0">
        <div className="h-14 px-5 flex items-center gap-2.5 border-b border-neutral-200">
          <div className="w-8 h-8 rounded-lg bg-primary-500 flex items-center justify-center">
            <Box className="w-4.5 h-4.5 text-white" />
          </div>
          <span className="text-base font-semibold text-neutral-800 tracking-tight">AgentClawBox</span>
        </div>

        <nav className="flex-1 py-2 px-3 space-y-0.5">
          <SidebarLink to="/" icon={<Box className="w-4 h-4" />} label="我的实例" />
          <SidebarLink to="/marketplace" icon={<Store className="w-4 h-4" />} label="应用市场" />
          <SidebarLink to="/help" icon={<LifeBuoy className="w-4 h-4" />} label="帮助中心" />
          <SidebarLink to="/about" icon={<UserCircle2 className="w-4 h-4" />} label="关于我" />
        </nav>

        <div className="px-5 py-3 text-caption text-neutral-400 border-t border-neutral-100">
          AgentClawBox v0.1.0
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto bg-neutral-50">
        <Outlet />
      </main>
    </div>
  );
}

function SidebarLink({ to, icon, label }: { to: string; icon: React.ReactNode; label: string }) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        `flex items-center gap-2.5 px-3 py-2 rounded-md text-body transition-colors duration-150 ${
          isActive
            ? "bg-primary-50 text-primary-500 font-medium"
            : "text-neutral-600 hover:bg-neutral-50 hover:text-neutral-800"
        }`
      }
    >
      {icon}
      {label}
    </NavLink>
  );
}
