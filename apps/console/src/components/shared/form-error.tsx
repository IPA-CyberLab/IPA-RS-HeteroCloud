import { Alert } from "@/components/ui";

interface FormErrorProps {
  message?: string | null;
}

export function FormError({ message }: FormErrorProps) {
  if (!message) return null;
  return <Alert type="error">{message}</Alert>;
}
