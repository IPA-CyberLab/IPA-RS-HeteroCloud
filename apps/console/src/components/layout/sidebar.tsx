import { NavLink } from "react-router-dom";
import { cn } from "@/lib/utils";
import {
  HeteroCloudMark,
  navigationSections,
} from "@/components/layout/navigation";

interface SidebarProps {
  onNavigate?: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-[#151719] text-zinc-100">
      <div className="flex h-16 shrink-0 items-center gap-3 border-b border-white/10 px-5">
        <span className="flex size-8 items-center justify-center rounded-[6px] bg-emerald-500 text-zinc-950">
          <HeteroCloudMark className="size-5" />
        </span>
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-white">
            HeteroCloud
          </div>
          <div className="text-xs text-zinc-400">管理コンソール</div>
        </div>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-3 py-4" aria-label="メイン">
        {navigationSections.map((section, sectionIndex) => (
          <div
            key={section.label ?? `primary-${sectionIndex}`}
            className={cn(sectionIndex > 0 && "mt-5")}
          >
            {section.label ? (
              <div className="mb-1 px-3 text-xs font-medium text-zinc-500">
                {section.label}
              </div>
            ) : null}
            <div className="space-y-1">
              {section.items.map((item) => {
                const Icon = item.icon;
                return (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    onClick={onNavigate}
                    className={({ isActive }) =>
                      cn(
                        "flex h-9 items-center gap-3 rounded-[6px] px-3 text-sm text-zinc-300 outline-none transition-colors hover:bg-white/8 hover:text-white focus-visible:ring-2 focus-visible:ring-emerald-400",
                        isActive &&
                          "bg-white/10 font-medium text-white before:h-4 before:w-0.5 before:rounded-full before:bg-emerald-400",
                      )
                    }
                  >
                    <Icon className="size-4 shrink-0" />
                    <span className="truncate">{item.label}</span>
                  </NavLink>
                );
              })}
            </div>
          </div>
        ))}
      </nav>

      <div className="shrink-0 border-t border-white/10 px-5 py-3 text-xs text-zinc-500">
        HeteroCloud Console
      </div>
    </div>
  );
}
