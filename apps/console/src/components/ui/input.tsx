import type { InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export function Input({ className, type, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      type={type}
      className={cn(
        "flex h-9 w-full rounded-[6px] border border-zinc-300 bg-white px-3 text-sm text-zinc-950 shadow-xs outline-none placeholder:text-zinc-400 focus:border-emerald-600 focus:ring-2 focus:ring-emerald-600/15 disabled:cursor-not-allowed disabled:bg-zinc-100 disabled:text-zinc-500",
        className,
      )}
      {...props}
    />
  );
}
