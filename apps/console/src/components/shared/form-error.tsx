import { CircleAlert } from "lucide-react";

interface FormErrorProps {
  message?: string | null;
}

export function FormError({ message }: FormErrorProps) {
  if (!message) return null;

  return (
    <div
      className="flex gap-2 border border-red-200 bg-red-50 px-3 py-2.5 text-sm text-red-800"
      role="alert"
    >
      <CircleAlert className="mt-0.5 size-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}
