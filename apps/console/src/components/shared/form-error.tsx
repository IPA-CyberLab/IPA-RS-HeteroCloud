import Alert from "@cloudscape-design/components/alert";

interface FormErrorProps {
  message?: string | null;
}

export function FormError({ message }: FormErrorProps) {
  if (!message) return null;
  return <Alert type="error">{message}</Alert>;
}
