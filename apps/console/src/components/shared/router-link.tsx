import Link, { type LinkProps } from "@cloudscape-design/components/link";
import { useNavigate } from "react-router-dom";

type RouterLinkProps = Omit<LinkProps, "href" | "onFollow"> & {
  to: string;
};

export function RouterLink({ to, ...props }: RouterLinkProps) {
  const navigate = useNavigate();
  return (
    <Link
      {...props}
      href={to}
      onFollow={(event) => {
        event.preventDefault();
        navigate(to);
      }}
    />
  );
}
