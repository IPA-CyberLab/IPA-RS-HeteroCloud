import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Circle,
  CircleAlert,
  CircleHelp,
  Copy,
  Download,
  ExternalLink,
  Folder,
  KeyRound,
  LayoutDashboard,
  LogOut,
  Menu,
  Moon,
  Network,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings,
  Shield,
  Sun,
  Terminal,
  Trash2,
  UserRound,
  X,
} from "lucide-react";

/** Common event shape for the platform's controlled forms. */
type Change<T> = { detail: T; preventDefault: () => void };
function change<T>(detail: T, event?: { preventDefault(): void }): Change<T> {
  return { detail, preventDefault: () => event?.preventDefault() };
}
type Props = { children?: ReactNode; [key: string]: any };
type Option = {
  value?: string;
  label?: string;
  description?: ReactNode;
  disabled?: boolean;
  options?: readonly Option[];
  filteringTags?: readonly string[];
  [key: string]: any;
};
const iconMap: Record<string, typeof Plus> = {
  "add-plus": Plus,
  add: Plus,
  refresh: RefreshCw,
  remove: Trash2,
  edit: Pencil,
  copy: Copy,
  close: X,
  check: Check,
  download: Download,
  external: ExternalLink,
  key: KeyRound,
  script: Terminal,
  terminal: Terminal,
  file: Folder,
  "file-open": Folder,
  "user-profile": UserRound,
  "user-profile-active": UserRound,
  folder: Folder,
  "sign-out": LogOut,
  "exit-full-screen": LogOut,
  settings: Settings,
  multiscreen: Server,
  "status-positive": CheckCircle2,
  "status-negative": CircleAlert,
  "status-pending": Circle,
  "light-dark": Moon,
  menu: Menu,
  search: Search,
};
function Icon({ name, size = 16 }: { name?: string; size?: number }) {
  const Glyph = name ? iconMap[name] : undefined;
  return Glyph ? <Glyph size={size} aria-hidden="true" /> : null;
}
const sizes: Record<string, string> = {
  xxs: ".25rem",
  xs: ".5rem",
  s: ".75rem",
  m: "1rem",
  l: "1.5rem",
  xl: "2rem",
  xxl: "2.75rem",
};
const space = (value?: string) => sizes[value ?? "m"] ?? "1rem";

export function SpaceBetween({
  children,
  size = "m",
  direction = "vertical",
  alignItems,
  ...props
}: Props) {
  return (
    <div
      className={`ui-stack ${direction === "horizontal" ? "ui-stack--row" : ""}`}
      style={{ gap: space(size), alignItems }}
      {...(props.id ? { id: props.id } : {})}
    >
      {children}
    </div>
  );
}
export function Box({
  children,
  variant,
  color,
  fontWeight,
  fontSize,
  textAlign,
  padding,
  margin,
  float,
  className = "",
}: Props) {
  const tag = (
    ["h1", "h2", "h3", "h4", "p", "pre", "code", "small", "strong"].includes(
      variant,
    )
      ? variant
      : "div"
  ) as keyof React.JSX.IntrinsicElements;
  const Tag = tag as any;
  return (
    <Tag
      className={`ui-box ${variant ? `ui-box--${variant}` : ""} ${color ? `ui-text--${color}` : ""} ${className}`}
      style={{
        fontWeight: fontWeight === "bold" ? 700 : undefined,
        fontSize: fontSize ? space(fontSize) : undefined,
        textAlign,
        padding: typeof padding === "string" ? space(padding) : undefined,
        margin:
          typeof margin === "object" && margin?.vertical
            ? `${space(margin.vertical)} 0`
            : undefined,
        float,
      }}
    >
      {children}
    </Tag>
  );
}
export function Header({
  children,
  variant = "h2",
  description,
  counter,
  actions,
}: Props) {
  const Tag = (variant === "h1" ? "h1" : variant === "h3" ? "h3" : "h2") as
    | "h1"
    | "h2"
    | "h3";
  return (
    <div className={`ui-section-header ui-section-header--${variant}`}>
      <div className="ui-section-header__copy">
        <Tag>
          {children}
          {counter && <span className="ui-counter">{counter}</span>}
        </Tag>
        {description && <p>{description}</p>}
      </div>
      {actions && <div className="ui-section-header__actions">{actions}</div>}
    </div>
  );
}
export function Container({ children, header }: Props) {
  return (
    <section className="card ui-card">
      <div className="ui-card__body">
        {header && <div className="ui-card__header">{header}</div>}
        {children}
      </div>
    </section>
  );
}
export function ContentLayout({ children, header }: Props) {
  return (
    <div className="ui-content-layout">
      {header && <div className="ui-content-layout__header">{header}</div>}
      {children}
    </div>
  );
}
export function ColumnLayout({ children, columns = 2 }: Props) {
  return (
    <div
      className="ui-columns"
      data-columns={Math.min(4, Math.max(1, Number(columns)))}
    >
      {children}
    </div>
  );
}

export interface LinkProps extends Props {
  href?: string;
  ariaLabel?: string;
  external?: boolean;
  onFollow?: (event: Change<{ href: string; external?: boolean }>) => void;
}
export function Link({
  children,
  href = "#",
  ariaLabel,
  external,
  externalIconAriaLabel,
  onFollow,
  ...props
}: LinkProps) {
  return (
    <a
      className="ui-link"
      href={href}
      aria-label={ariaLabel}
      target={external ? "_blank" : props.target}
      rel={external ? "noopener noreferrer" : props.rel}
      onClick={(event: MouseEvent<HTMLAnchorElement>) => {
        if (
          onFollow &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.shiftKey &&
          !event.altKey &&
          event.button === 0
        )
          onFollow(change({ href, external }, event));
      }}
    >
      {children}
      {external && (
        <ExternalLink size={13} aria-label={externalIconAriaLabel} />
      )}
    </a>
  );
}
export function Button({
  children,
  variant,
  iconName,
  ariaLabel,
  href,
  external,
  target,
  disabled,
  loading,
  fullWidth,
  formAction,
  onClick,
  ...props
}: Props) {
  const inline = variant === "inline-link" || variant === "link";
  const classes = inline
    ? "ui-inline-link"
    : `btn ui-btn ${variant === "primary" ? "btn-primary" : variant === "icon" ? "btn-ghost btn-square" : variant === "normal" || !variant ? "btn-outline" : "btn-ghost"} ${fullWidth ? "ui-btn--full" : ""}`;
  const inner = (
    <>
      {loading ? (
        <span
          className="loading loading-spinner loading-xs"
          aria-hidden="true"
        />
      ) : (
        <Icon name={iconName} />
      )}
      {children}
    </>
  );
  if (href)
    return (
      <a
        href={disabled || loading ? undefined : href}
        className={classes}
        aria-label={ariaLabel}
        aria-disabled={disabled || loading || undefined}
        target={target || (external ? "_blank" : undefined)}
        rel={
          target === "_blank" || external ? "noopener noreferrer" : undefined
        }
        onClick={onClick}
      >
        {inner}
      </a>
    );
  return (
    <button
      type={formAction === "submit" ? "submit" : "button"}
      className={classes}
      aria-label={ariaLabel}
      disabled={disabled || loading}
      onClick={onClick}
      title={props.title}
    >
      {inner}
    </button>
  );
}
export function Badge({ children, color = "blue" }: Props) {
  const map: Record<string, string> = {
    green: "badge-success",
    red: "badge-error",
    blue: "badge-info",
    grey: "badge-neutral",
    grey2: "badge-neutral",
    yellow: "badge-warning",
    severityHigh: "badge-error",
    severityMedium: "badge-warning",
  };
  return (
    <span className={`badge badge-soft ${map[color] ?? "badge-neutral"}`}>
      {children}
    </span>
  );
}
export namespace StatusIndicatorProps {
  export type Type =
    | "success"
    | "error"
    | "warning"
    | "info"
    | "loading"
    | "pending"
    | "in-progress"
    | "stopped";
}
export function StatusIndicator({ children, type = "info" }: Props) {
  const kind =
    type === "success"
      ? "success"
      : type === "error"
        ? "error"
        : type === "warning"
          ? "warning"
          : type === "loading" || type === "in-progress" || type === "pending"
            ? "loading"
            : "neutral";
  return (
    <span className={`ui-status ui-status--${kind}`}>
      {kind === "loading" ? (
        <span
          className="loading loading-spinner loading-xs"
          aria-hidden="true"
        />
      ) : (
        <span className="ui-status__dot" aria-hidden="true" />
      )}
      {children}
    </span>
  );
}
export function Spinner({ size = "normal" }: Props) {
  return (
    <span
      className={`loading loading-spinner ${size === "big" ? "loading-md" : "loading-sm"}`}
      aria-hidden="true"
    />
  );
}
export function Alert({
  children,
  type = "info",
  header,
  action,
  dismissible,
  onDismiss,
}: Props) {
  const IconComponent =
    type === "error"
      ? CircleAlert
      : type === "warning"
        ? CircleAlert
        : type === "success"
          ? CheckCircle2
          : CircleHelp;
  return (
    <div
      className={`alert ui-alert ui-alert--${type}`}
      role={type === "error" ? "alert" : "status"}
    >
      <IconComponent size={19} aria-hidden="true" />
      <div className="ui-alert__copy">
        {header && <strong>{header}</strong>}
        {children && <div>{children}</div>}
      </div>
      {action && <div>{action}</div>}
      {dismissible && (
        <button
          type="button"
          className="btn btn-ghost btn-square btn-sm"
          aria-label="ダイアログを閉じる"
          onClick={onDismiss}
        >
          <X size={16} />
        </button>
      )}
    </div>
  );
}
export namespace FlashbarProps {
  export interface MessageDefinition extends Props {
    id?: string;
    content?: ReactNode;
    type?: string;
    dismissible?: boolean;
    onDismiss?: () => void;
  }
}
export function Flashbar({ items = [] }: Props) {
  return (
    <div className="ui-flashbar" aria-live="polite">
      {items.map((item: FlashbarProps.MessageDefinition, index: number) => (
        <Alert
          key={item.id ?? index}
          type={item.type}
          dismissible={item.dismissible}
          onDismiss={item.onDismiss}
        >
          {item.content}
        </Alert>
      ))}
    </div>
  );
}

type FieldContext = {
  id: string;
  descriptionId?: string;
  errorId?: string;
} | null;
const Field = createContext<FieldContext>(null);
export function FormField({
  children,
  label,
  description,
  constraintText,
  errorText,
}: Props) {
  const id = useId(),
    descriptionId =
      description || constraintText ? `${id}-description` : undefined,
    errorId = errorText ? `${id}-error` : undefined;
  return (
    <div className="fieldset ui-field">
      <div className="ui-field__label">
        {typeof label === "string" ? (
          <label htmlFor={id}>{label}</label>
        ) : (
          label
        )}
      </div>
      {description && (
        <p id={descriptionId} className="ui-field__description">
          {description}
        </p>
      )}
      <Field.Provider value={{ id, descriptionId, errorId }}>
        {children}
      </Field.Provider>
      {constraintText && (
        <p
          className="ui-field__hint"
          id={!description ? descriptionId : undefined}
        >
          {constraintText}
        </p>
      )}
      {errorText && (
        <p id={errorId} className="ui-field__error" role="alert">
          {errorText}
        </p>
      )}
    </div>
  );
}
function fieldAttributes(field: FieldContext, ariaLabel?: string) {
  return {
    id: field?.id,
    "aria-label": ariaLabel,
    "aria-describedby":
      [field?.descriptionId, field?.errorId].filter(Boolean).join(" ") ||
      undefined,
    "aria-invalid": field?.errorId ? true : undefined,
  };
}
export function Input({
  value = "",
  onChange,
  onKeyDown,
  ariaLabel,
  nativeInputAttributes,
  invalid,
  ...props
}: Props & {
  onChange?: (event: Change<{ value: string }>) => void;
  onKeyDown?: (event: KeyboardEvent<HTMLInputElement>) => void;
}) {
  const field = useContext(Field);
  return (
    <input
      {...nativeInputAttributes}
      {...fieldAttributes(field, ariaLabel)}
      className={`input ui-input ${invalid || field?.errorId ? "input-error" : ""}`}
      value={value}
      type={props.type || "text"}
      inputMode={props.inputMode}
      step={props.step}
      autoComplete={props.autoComplete}
      disabled={props.disabled}
      readOnly={props.readOnly}
      placeholder={props.placeholder}
      onKeyDown={onKeyDown}
      onChange={(event) =>
        onChange?.(change({ value: event.currentTarget.value }, event))
      }
    />
  );
}
export function Textarea({
  value = "",
  onChange,
  ariaLabel,
  ...props
}: Props & { onChange?: (event: Change<{ value: string }>) => void }) {
  const field = useContext(Field);
  return (
    <textarea
      {...fieldAttributes(field, ariaLabel)}
      className={`textarea ui-textarea ${field?.errorId ? "textarea-error" : ""}`}
      value={value}
      rows={props.rows ?? 3}
      disabled={props.disabled}
      placeholder={props.placeholder}
      onChange={(event) =>
        onChange?.(change({ value: event.currentTarget.value }, event))
      }
    />
  );
}
export function Toggle({
  checked = false,
  onChange,
  children,
  disabled,
  ariaLabel,
}: Props & { onChange?: (event: Change<{ checked: boolean }>) => void }) {
  const field = useContext(Field);
  return (
    <label className="ui-toggle">
      <input
        type="checkbox"
        id={field?.id}
        className="toggle toggle-primary toggle-sm"
        checked={checked}
        disabled={disabled}
        aria-label={ariaLabel}
        aria-describedby={field?.descriptionId}
        onChange={(event) =>
          onChange?.(change({ checked: event.currentTarget.checked }, event))
        }
      />
      <span>{children}</span>
    </label>
  );
}
export function Form({ children, header, actions, errorText }: Props) {
  return (
    <div className="ui-form">
      {header}
      {errorText && (
        <div className="alert ui-alert ui-alert--error" role="alert">
          {errorText}
        </div>
      )}
      {children}
      {actions && <div className="ui-form__actions">{actions}</div>}
    </div>
  );
}

function flattenOptions(options: readonly Option[] = []): Option[] {
  return options.flatMap((option) =>
    option.options ? flattenOptions(option.options) : [option],
  );
}
export function Select({
  options = [],
  selectedOption,
  onChange,
  ariaLabel,
  placeholder = "選択してください",
  disabled,
  statusType,
  recoveryText,
  onLoadItems,
}: Props & {
  options?: readonly Option[];
  selectedOption?: Option | null;
  onChange?: (event: Change<{ selectedOption: Option }>) => void;
}) {
  const field = useContext(Field),
    all = flattenOptions(options),
    selected = selectedOption?.value ?? "";
  const known = all.some((option) => option.value === selected),
    hasEmpty = all.some((option) => option.value === "");
  return (
    <div className="ui-select-wrap">
      <select
        {...fieldAttributes(field, ariaLabel)}
        className="select ui-select"
        value={selected}
        disabled={disabled}
        onChange={(event) => {
          const option = all.find(
            (item) => item.value === event.currentTarget.value,
          );
          if (option) onChange?.(change({ selectedOption: option }, event));
        }}
      >
        {!hasEmpty && (
          <option value="" disabled>
            {statusType === "loading" ? "読み込み中" : placeholder}
          </option>
        )}
        {selected && !known && (
          <option value={selected}>{selectedOption?.label ?? selected}</option>
        )}
        {all.map((option, index) => (
          <option
            key={`${option.value}-${index}`}
            value={option.value ?? ""}
            disabled={option.disabled}
          >
            {option.label ?? option.value}
            {option.labelTag
              ? ` · ${option.labelTag}${typeof option.description === "string" ? ` · ${option.description}` : ""}`
              : ""}
          </option>
        ))}
      </select>
      {statusType === "error" && onLoadItems && (
        <button
          type="button"
          className="ui-inline-link"
          onClick={() =>
            onLoadItems(change({ filteringText: "", firstPage: true }))
          }
        >
          {recoveryText ?? "再試行"}
        </button>
      )}
    </div>
  );
}
export function Multiselect({
  options = [],
  selectedOptions = [],
  onChange,
  ariaLabel,
  placeholder = "選択してください",
  disabled,
  filteringPlaceholder = "検索",
}: Props & {
  options?: readonly Option[];
  selectedOptions?: Option[];
  onChange?: (event: Change<{ selectedOptions: Option[] }>) => void;
}) {
  const field = useContext(Field),
    [open, setOpen] = useState(false),
    [filter, setFilter] = useState("");
  const root = useRef<HTMLDivElement>(null),
    all = flattenOptions(options);
  useEffect(() => {
    if (!open) return;
    const handler = (event: globalThis.MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", handler);
    return () => document.removeEventListener("pointerdown", handler);
  }, [open]);
  const toggle = (option: Option) => {
    if (disabled || option.disabled) return;
    const exists = selectedOptions.some((item) => item.value === option.value);
    onChange?.(
      change({
        selectedOptions: exists
          ? selectedOptions.filter((item) => item.value !== option.value)
          : [...selectedOptions, option],
      }),
    );
  };
  const shown = all.filter((option) =>
    `${option.label ?? ""} ${option.value ?? ""}`
      .toLowerCase()
      .includes(filter.toLowerCase()),
  );
  return (
    <div className="ui-multiselect" ref={root}>
      <button
        type="button"
        {...fieldAttributes(field, ariaLabel)}
        className="select ui-multiselect__trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {selectedOptions.length
          ? `${selectedOptions.length}件を選択`
          : placeholder}
      </button>
      {selectedOptions.length > 0 && (
        <div className="ui-selected-options">
          {selectedOptions.map((option, index) => (
            <span
              className="badge badge-soft badge-primary"
              key={`${option.value}-${index}`}
            >
              {option.label ?? option.value}
              <button
                type="button"
                disabled={disabled}
                aria-label={`${option.label ?? option.value} を削除`}
                onClick={() => toggle(option)}
              >
                <X size={13} />
              </button>
            </span>
          ))}
        </div>
      )}
      {open && (
        <div className="ui-option-menu">
          <input
            className="input ui-input input-sm"
            aria-label={filteringPlaceholder}
            placeholder={filteringPlaceholder}
            value={filter}
            onChange={(e) => setFilter(e.currentTarget.value)}
          />
          <div role="listbox" aria-multiselectable="true">
            {shown.map((option, index) => {
              const selected = selectedOptions.some(
                (item) => item.value === option.value,
              );
              return (
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  disabled={option.disabled}
                  key={`${option.value}-${index}`}
                  onClick={() => toggle(option)}
                >
                  <span
                    className={`ui-option-check ${selected ? "is-selected" : ""}`}
                  >
                    {selected && <Check size={13} />}
                  </span>
                  <span>
                    {option.label ?? option.value}
                    {option.description && <small>{option.description}</small>}
                  </span>
                </button>
              );
            })}
            {!shown.length && (
              <p className="ui-empty-inline">該当する項目がありません。</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
export function SegmentedControl({
  options = [],
  selectedId,
  onChange,
  label,
}: Props & { onChange?: (event: Change<{ selectedId: string }>) => void }) {
  return (
    <div className="join ui-segmented" role="group" aria-label={label}>
      {options.map((option: any) => (
        <button
          type="button"
          key={option.id}
          className={`btn btn-sm join-item ${selectedId === option.id ? "btn-primary" : "btn-ghost"}`}
          aria-pressed={selectedId === option.id}
          disabled={option.disabled}
          onClick={() => onChange?.(change({ selectedId: option.id }))}
        >
          {option.text}
        </button>
      ))}
    </div>
  );
}
export function Autosuggest({
  options = [],
  value = "",
  onChange,
  onSelect,
  ariaLabel,
  placeholder,
  empty,
}: Props & {
  onChange?: (event: Change<{ value: string }>) => void;
  onSelect?: (event: Change<{ value: string }>) => void;
}) {
  const [open, setOpen] = useState(false),
    [active, setActive] = useState(-1),
    id = useId();
  const root = useRef<HTMLDivElement>(null);
  const shown: Option[] = options.filter((option: Option) =>
    [
      option.label,
      option.value,
      option.description,
      ...(option.filteringTags ?? []),
    ]
      .filter((v) => typeof v === "string")
      .join(" ")
      .toLowerCase()
      .includes(value.toLowerCase()),
  );
  useEffect(() => {
    if (!open) return;
    const handler = (event: globalThis.MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", handler);
    return () => document.removeEventListener("pointerdown", handler);
  }, [open]);
  const choose = (option: Option) => {
    setOpen(false);
    onSelect?.(change({ value: option.value ?? "" }));
  };
  return (
    <div className="ui-autosuggest" ref={root}>
      <Search size={17} className="ui-autosuggest__icon" aria-hidden="true" />
      <input
        className="input ui-input ui-autosuggest__input"
        value={value}
        placeholder={placeholder}
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={open}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={
          open && shown[active] ? `${id}-${active}` : undefined
        }
        onFocus={() => setOpen(true)}
        onChange={(event) => {
          setActive(-1);
          setOpen(true);
          onChange?.(change({ value: event.currentTarget.value }, event));
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            setActive(Math.min(active + 1, shown.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive(Math.max(0, active - 1));
          } else if (event.key === "Enter" && open && shown[active]) {
            event.preventDefault();
            choose(shown[active]);
          } else if (event.key === "Escape") setOpen(false);
        }}
      />
      {open && (
        <div className="ui-option-menu" id={`${id}-list`} role="listbox">
          {shown.map((option, index) => (
            <button
              type="button"
              role="option"
              id={`${id}-${index}`}
              aria-selected={active === index}
              key={option.value ?? index}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => choose(option)}
            >
              <span>
                {option.label ?? option.value}
                {option.description && <small>{option.description}</small>}
              </span>
            </button>
          ))}
          {!shown.length && (
            <p className="ui-empty-inline">
              {empty ?? "該当する項目がありません。"}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

let modalDepth = 0;
let previousOverflow = "";
const modalStack: string[] = [];
export function Modal({
  children,
  visible,
  header,
  footer,
  onDismiss,
  size = "medium",
}: Props) {
  const id = useId(),
    dialog = useRef<HTMLDivElement>(null),
    titleId = `${id}-title`;
  const dismissRef = useRef(onDismiss);
  dismissRef.current = onDismiss;
  useEffect(() => {
    if (!visible) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    if (modalDepth++ === 0) {
      previousOverflow = document.body.style.overflow;
      document.body.style.overflow = "hidden";
    }
    modalStack.push(id);
    const timer = window.setTimeout(
      () =>
        (
          dialog.current?.querySelector<HTMLElement>(
            ".ui-modal__body input:not([disabled]),.ui-modal__body select:not([disabled]),.ui-modal__body textarea:not([disabled])",
          ) ??
          dialog.current?.querySelector<HTMLElement>("button:not([disabled])")
        )?.focus(),
      0,
    );
    const keydown = (event: globalThis.KeyboardEvent) => {
      if (modalStack.at(-1) !== id) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        dismissRef.current?.();
        return;
      }
      if (event.key !== "Tab" || !dialog.current) return;
      const items = [
        ...dialog.current.querySelectorAll<HTMLElement>(
          'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])',
        ),
      ].filter((el) => !el.closest("[hidden]"));
      if (!items.length) {
        event.preventDefault();
        dialog.current.focus();
        return;
      }
      const first = items[0],
        last = items[items.length - 1];
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          !dialog.current.contains(document.activeElement))
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", keydown, true);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("keydown", keydown, true);
      const index = modalStack.indexOf(id);
      if (index >= 0) modalStack.splice(index, 1);
      if (--modalDepth === 0) document.body.style.overflow = previousOverflow;
      previousFocus?.isConnected && previousFocus.focus();
    };
  }, [visible, id]);
  if (!visible) return null;
  return createPortal(
    <div
      className="modal modal-open ui-modal"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onDismiss?.();
      }}
    >
      <div
        className={`modal-box ui-modal__box ui-modal__box--${size}`}
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <div className="ui-modal__header">
          <h2 id={titleId}>{header}</h2>
          <button
            type="button"
            className="btn btn-ghost btn-square btn-sm"
            aria-label="ダイアログを閉じる"
            onClick={onDismiss}
          >
            <X size={18} />
          </button>
        </div>
        <div className="ui-modal__body">{children}</div>
        {footer && <div className="ui-modal__footer">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export namespace SideNavigationProps {
  export interface Item extends Props {
    type: "link" | "section" | "divider";
    text?: string;
    href?: string;
    items?: Item[];
    info?: ReactNode;
  }
}
function navigationIcon(href = "") {
  if (href.includes("overview")) return LayoutDashboard;
  if (href.includes("node") || href.includes("gpu")) return Server;
  if (
    href.includes("vpc") ||
    href.includes("topology") ||
    href.includes("path") ||
    href.includes("route")
  )
    return Network;
  if (href.includes("acl") || href.includes("iam") || href.includes("audit"))
    return Shield;
  if (href.includes("settings")) return Settings;
  if (href.includes("cli")) return Terminal;
  if (href.includes("flow") || href.includes("transport")) return Activity;
  if (
    href.includes("registry") ||
    href.includes("bucket") ||
    href.includes("backup")
  )
    return Folder;
  return Circle;
}
export function SideNavigation({
  header,
  items = [],
  activeHref,
  onFollow,
}: Props & {
  items?: SideNavigationProps.Item[];
  onFollow?: (event: Change<{ href: string; external?: boolean }>) => void;
}) {
  const follow = (
    href: string,
    external: boolean | undefined,
    event: MouseEvent<HTMLAnchorElement>,
  ) => {
    if (
      onFollow &&
      !event.metaKey &&
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.altKey &&
      event.button === 0
    )
      onFollow(change({ href, external }, event));
  };
  const render = (item: SideNavigationProps.Item, index: number) => {
    if (item.type === "divider")
      return <li className="ui-nav-divider" key={`divider-${index}`} />;
    if (item.type === "section")
      return (
        <li className="ui-nav-section" key={item.text ?? index}>
          <details open>
            <summary>
              {item.text}
              <ChevronDown size={15} aria-hidden="true" />
            </summary>
            <ul>{(item.items ?? []).map(render)}</ul>
          </details>
        </li>
      );
    const href = item.href ?? "#",
      active =
        activeHref === href ||
        (href !== "/overview" &&
          href !== "#/overview" &&
          activeHref?.startsWith(href + "/"));
    const Glyph = navigationIcon(href);
    return (
      <li key={href}>
        <a
          href={href}
          className={active ? "is-active" : ""}
          aria-current={active ? "page" : undefined}
          onClick={(event) => follow(href, item.external, event)}
        >
          <Glyph size={17} aria-hidden="true" />
          <span>{item.text}</span>
          {item.info != null && (
            <span className="ui-nav-info">{item.info}</span>
          )}
        </a>
      </li>
    );
  };
  return (
    <nav className="ui-side-nav" aria-label="メインナビゲーション">
      {header?.text && <div className="ui-side-nav__title">{header.text}</div>}
      <ul className="menu ui-menu">{items.map(render)}</ul>
    </nav>
  );
}
export function BreadcrumbGroup({
  items = [],
  onFollow,
}: Props & { onFollow?: (event: Change<{ href: string }>) => void }) {
  const normalized = items.filter(
    (item: any, index: number) =>
      index === items.length - 1 || item.href !== items[index + 1]?.href,
  );
  return (
    <nav aria-label="パンくずリスト" className="ui-breadcrumbs">
      <ol>
        {normalized.map((item: any, index: number) => (
          <li key={`${item.href}-${index}`}>
            {index === normalized.length - 1 ? (
              <span aria-current="page">{item.text}</span>
            ) : (
              <a
                href={item.href}
                onClick={(event) => {
                  if (
                    onFollow &&
                    !event.metaKey &&
                    !event.ctrlKey &&
                    !event.shiftKey &&
                    !event.altKey &&
                    event.button === 0
                  )
                    onFollow(change({ href: item.href }, event));
                }}
              >
                {item.text}
              </a>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}
export namespace TopNavigationProps {
  export interface Utility extends Props {
    type: "button" | "menu-dropdown";
    text?: string;
    ariaLabel?: string;
    items?: any[];
    onItemClick?: (event: Change<{ id: string }>) => void;
    onClick?: () => void;
  }
}
export function TopNavigation({
  identity,
  search,
  utilities = [],
}: Props & {
  identity?: {
    href?: string;
    title?: string;
    onFollow?: (event: Change<{ href: string }>) => void;
  };
  utilities?: TopNavigationProps.Utility[];
}) {
  const [mobileSearch, setMobileSearch] = useState(false);
  const cloud = String(identity?.title ?? "").startsWith("HeteroCloud");
  const [theme, setTheme] = useState(() =>
    typeof document !== "undefined" &&
    (localStorage.getItem("heterocloud_theme") ||
      document.documentElement.dataset.theme) === "dark"
      ? "dark"
      : "light",
  );
  useEffect(() => {
    if (!cloud) return;
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("heterocloud_theme", theme);
  }, [theme, cloud]);
  useEffect(() => {
    const close = (event: globalThis.MouseEvent) => {
      if ((event.target as Element)?.closest?.(".ui-top-menu")) return;
      document
        .querySelectorAll<HTMLDetailsElement>(".ui-top-menu[open]")
        .forEach((menu) => (menu.open = false));
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <div className="navbar ui-topnav">
      <a
        className="ui-brand"
        href={identity?.href ?? "/"}
        onClick={(event) => {
          if (
            identity?.onFollow &&
            !event.metaKey &&
            !event.ctrlKey &&
            !event.shiftKey &&
            !event.altKey &&
            event.button === 0
          )
            identity.onFollow(change({ href: identity.href ?? "/" }, event));
        }}
      >
        <span className="ui-brand__mark" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
        </span>
        <strong>{identity?.title}</strong>
      </a>
      <div className={`ui-topnav__search ${mobileSearch ? "is-open" : ""}`}>
        {search}
      </div>
      <div className="ui-topnav__utilities">
        {search && (
          <button
            type="button"
            className="btn btn-ghost btn-square ui-topnav__mobile-search"
            aria-label={mobileSearch ? "検索を閉じる" : "検索を開く"}
            onClick={() => setMobileSearch(!mobileSearch)}
          >
            {mobileSearch ? <X size={18} /> : <Search size={18} />}
          </button>
        )}
        {cloud && (
          <button
            type="button"
            className="btn btn-ghost btn-square ui-topnav__theme"
            aria-label={
              theme === "dark"
                ? "ライトモードに切り替え"
                : "ダークモードに切り替え"
            }
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          >
            {theme === "dark" ? <Sun size={18} /> : <Moon size={18} />}
          </button>
        )}
        {utilities.map((utility, index) =>
          utility.type === "button" ? (
            <button
              type="button"
              key={index}
              className="btn btn-ghost ui-topnav__utility"
              aria-label={utility.ariaLabel || utility.text}
              onClick={utility.onClick}
            >
              {utility.iconName === "light-dark" &&
              String(utility.ariaLabel).includes("ライト") ? (
                <Sun size={17} />
              ) : (
                <Icon name={utility.iconName} />
              )}
              <span>{utility.text}</span>
            </button>
          ) : (
            <details className="dropdown dropdown-end ui-top-menu" key={index}>
              <summary
                className="btn btn-ghost ui-topnav__utility"
                aria-label={utility.ariaLabel || utility.text}
              >
                <Icon name={utility.iconName} />
                {utility.ariaLabel === "操作対象の組織" && (
                  <span className="ui-topnav__utility-label">組織</span>
                )}
                <span className="ui-topnav__utility-text">{utility.text}</span>
                <ChevronDown size={14} aria-hidden="true" />
              </summary>
              <ul className="dropdown-content menu ui-dropdown" role="menu">
                {utility.description && (
                  <li className="ui-dropdown__description">
                    {utility.description}
                  </li>
                )}
                {(utility.items ?? []).map((item: any) => (
                  <li key={item.id}>
                    <button
                      role="menuitem"
                      type="button"
                      onClick={(event) => {
                        event.currentTarget
                          .closest("details")
                          ?.removeAttribute("open");
                        utility.onItemClick?.(change({ id: item.id }, event));
                      }}
                    >
                      <Icon name={item.iconName} />
                      {item.text}
                    </button>
                  </li>
                ))}
              </ul>
            </details>
          ),
        )}
      </div>
    </div>
  );
}
export function AppLayout({
  navigationOpen = true,
  onNavigationChange,
  navigation,
  breadcrumbs,
  notifications,
  content,
  ariaLabels = {},
}: Props & {
  onNavigationChange?: (event: Change<{ open: boolean }>) => void;
}) {
  return (
    <div className={`ui-layout ${navigationOpen ? "is-nav-open" : ""}`}>
      <aside
        className="ui-layout__sidebar"
        aria-label={ariaLabels.navigation}
        onClickCapture={(event) => {
          if (
            window.innerWidth < 1024 &&
            (event.target as Element).closest("a")
          )
            onNavigationChange?.(change({ open: false }));
        }}
      >
        {navigation}
      </aside>
      {navigationOpen && (
        <button
          className="ui-layout__backdrop"
          type="button"
          aria-label={ariaLabels.navigationClose ?? "ナビゲーションを閉じる"}
          onClick={() => onNavigationChange?.(change({ open: false }))}
        />
      )}
      <main className="ui-layout__main">
        <div className="ui-layout__content">
          <div className="ui-layout__crumbrow">
            <button
              type="button"
              className="btn btn-ghost btn-square btn-sm ui-layout__menu-toggle"
              aria-label={
                navigationOpen
                  ? (ariaLabels.navigationClose ?? "ナビゲーションを閉じる")
                  : (ariaLabels.navigationToggle ?? "ナビゲーションを開く")
              }
              onClick={() =>
                onNavigationChange?.(change({ open: !navigationOpen }))
              }
            >
              <Menu size={18} />
            </button>
            {breadcrumbs}
          </div>
          {notifications}
          {content}
        </div>
      </main>
    </div>
  );
}
export function ExpandableSection({ headerText, children }: Props) {
  return (
    <details className="collapse ui-expandable">
      <summary className="collapse-title">
        {headerText}
        <ChevronDown size={16} />
      </summary>
      <div className="collapse-content">{children}</div>
    </details>
  );
}

export namespace TableProps {
  export interface ColumnDefinition<T> extends Props {
    id?: string;
    header?: ReactNode;
    cell?: (item: T) => ReactNode;
    sortingField?: string;
    sortingComparator?: (a: T, b: T) => number;
  }
}
interface TableBase<T> extends Props {
  trackBy?: string | ((item: T) => string);
  items?: T[];
  columnDefinitions?: TableProps.ColumnDefinition<T>[];
  onRowClick?: (event: Change<{ item: T }>) => void;
  onSortingChange?: (
    event: Change<{
      sortingColumn: TableProps.ColumnDefinition<T>;
      isDescending: boolean;
    }>,
  ) => void;
}
export function Table<T = any>({
  items = [],
  columnDefinitions = [],
  header,
  filter,
  pagination,
  empty,
  loading,
  loadingText,
  onRowClick,
  onSortingChange,
  sortingColumn,
  sortingDescending,
  columnDisplay,
  ariaLabels,
  trackBy,
}: TableBase<T>) {
  const [ownSort, setOwnSort] = useState<{
    column: TableProps.ColumnDefinition<T>;
    desc: boolean;
  } | null>(null);
  const visible = columnDefinitions.filter(
    (col) =>
      !columnDisplay ||
      columnDisplay.find((d: any) => String(d.id) === String(col.id))
        ?.visible !== false,
  );
  const sorted =
    !onSortingChange && ownSort
      ? [...items].sort((a, b) => {
          const c = ownSort.column;
          const value = c.sortingComparator
            ? c.sortingComparator(a, b)
            : String(
                (a as any)[c.sortingField ?? c.id ?? ""] ?? "",
              ).localeCompare(
                String((b as any)[c.sortingField ?? c.id ?? ""] ?? ""),
                "ja",
                { numeric: true },
              );
          return ownSort.desc ? -value : value;
        })
      : items;
  return (
    <div className="ui-table-card card">
      <div className="ui-table-card__header">
        {header}
        {filter}
      </div>
      <div className="ui-table-scroll" tabIndex={0}>
        <table
          className="table ui-table"
          aria-label={ariaLabels?.tableLabel ?? "リソース一覧"}
        >
          <thead>
            <tr>
              {visible.map((column, index) => {
                const sortable = Boolean(
                  column.sortingField || column.sortingComparator,
                );
                const active =
                  (sortingColumn?.id ?? ownSort?.column.id) === column.id;
                const descending = onSortingChange
                  ? sortingDescending
                  : ownSort?.desc;
                return (
                  <th
                    key={column.id ?? index}
                    scope="col"
                    aria-sort={
                      active
                        ? descending
                          ? "descending"
                          : "ascending"
                        : undefined
                    }
                  >
                    {sortable ? (
                      <button
                        type="button"
                        className="ui-sort"
                        onClick={() => {
                          const next = active ? !descending : false;
                          if (onSortingChange)
                            onSortingChange(
                              change({
                                sortingColumn: column,
                                isDescending: next,
                              }),
                            );
                          else setOwnSort({ column, desc: next });
                        }}
                      >
                        {column.header}
                        {active ? (
                          descending ? (
                            <ArrowDown size={14} />
                          ) : (
                            <ArrowUp size={14} />
                          )
                        ) : (
                          <span className="ui-sort__hint">↕</span>
                        )}
                      </button>
                    ) : (
                      column.header
                    )}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {!loading &&
              sorted.map((item, index) => (
                <tr
                  key={
                    typeof trackBy === "function"
                      ? trackBy(item)
                      : typeof trackBy === "string"
                        ? String((item as any)[trackBy] ?? index)
                        : index
                  }
                  className={onRowClick ? "ui-table__row--clickable" : ""}
                  onClick={(event) => {
                    if (
                      !(event.target as Element).closest(
                        "a,button,input,select,textarea,[role=button]",
                      )
                    )
                      onRowClick?.(change({ item }, event));
                  }}
                >
                  {visible.map((column, i) => (
                    <td key={column.id ?? i}>
                      {column.cell ? column.cell(item) : null}
                    </td>
                  ))}
                </tr>
              ))}
          </tbody>
        </table>
      </div>
      {loading ? (
        <div className="ui-table__empty">
          <Spinner /> {loadingText ?? "読み込んでいます"}
        </div>
      ) : !sorted.length ? (
        <div className="ui-table__empty">{empty ?? "データがありません"}</div>
      ) : null}
      {pagination && <div className="ui-table__footer">{pagination}</div>}
    </div>
  );
}
export function TextFilter({
  filteringText = "",
  filteringPlaceholder,
  filteringAriaLabel,
  onChange,
  countText,
}: Props & { onChange?: (event: Change<{ filteringText: string }>) => void }) {
  return (
    <div className="ui-table-filter">
      <div className="ui-search-input">
        <Search size={16} aria-hidden="true" />
        <input
          className="input ui-input"
          value={filteringText}
          aria-label={filteringAriaLabel ?? filteringPlaceholder ?? "検索"}
          placeholder={filteringPlaceholder}
          onChange={(event) =>
            onChange?.(
              change({ filteringText: event.currentTarget.value }, event),
            )
          }
        />
      </div>
      {countText && <span className="ui-muted">{countText}</span>}
    </div>
  );
}
export function Pagination({
  currentPageIndex = 1,
  pagesCount = 1,
  onChange,
  ariaLabels = {},
}: Props & {
  onChange?: (event: Change<{ currentPageIndex: number }>) => void;
  ariaLabels?: {
    nextPageLabel?: string;
    previousPageLabel?: string;
    pageLabel?: (page: number) => string;
  };
}) {
  return (
    <nav className="join ui-pagination" aria-label="ページ移動">
      <button
        type="button"
        className="btn btn-ghost btn-sm join-item"
        disabled={currentPageIndex <= 1}
        aria-disabled={currentPageIndex <= 1}
        aria-label={ariaLabels.previousPageLabel ?? "前のページ"}
        onClick={() =>
          onChange?.(change({ currentPageIndex: currentPageIndex - 1 }))
        }
      >
        <ChevronLeft size={16} />
      </button>
      <span className="ui-pagination__count">
        {currentPageIndex} / {pagesCount}
      </span>
      <button
        type="button"
        className="btn btn-ghost btn-sm join-item"
        disabled={currentPageIndex >= pagesCount}
        aria-disabled={currentPageIndex >= pagesCount}
        aria-label={ariaLabels.nextPageLabel ?? "次のページ"}
        onClick={() =>
          onChange?.(change({ currentPageIndex: currentPageIndex + 1 }))
        }
      >
        <ChevronRight size={16} />
      </button>
    </nav>
  );
}
export function Cards<T = any>({
  items = [],
  header,
  empty,
  cardDefinition,
  trackBy,
}: Props & {
  items?: T[];
  cardDefinition: {
    header: (item: T) => ReactNode;
    sections?: {
      id: string;
      header?: ReactNode;
      content: (item: T) => ReactNode;
    }[];
  };
}) {
  return (
    <section className="ui-card-collection">
      {header}
      {items.length ? (
        <div className="ui-cards-grid">
          {items.map((item, index) => (
            <article
              className="card ui-service-card"
              key={
                typeof trackBy === "string"
                  ? String((item as any)[trackBy] ?? index)
                  : index
              }
            >
              <div className="ui-service-card__title">
                {cardDefinition.header(item)}
                <ChevronRight size={16} aria-hidden="true" />
              </div>
              {cardDefinition.sections?.map((section) => (
                <div className="ui-service-card__section" key={section.id}>
                  {section.header && <span>{section.header}</span>}
                  <div>{section.content(item)}</div>
                </div>
              ))}
            </article>
          ))}
        </div>
      ) : (
        <div className="ui-card-collection__empty">{empty}</div>
      )}
    </section>
  );
}
export function KeyValuePairs({ items = [], columns = 2 }: Props) {
  return (
    <dl className="ui-key-values" data-columns={columns}>
      {items.filter(Boolean).map((item: any, index: number) => (
        <div key={index}>
          <dt>{item.label}</dt>
          <dd>{item.value ?? "—"}</dd>
        </div>
      ))}
    </dl>
  );
}
export function ProgressBar({
  value = 0,
  label,
  description,
  additionalInfo,
  status,
}: Props) {
  return (
    <div className="ui-progress">
      <div className="ui-progress__label">
        <strong>{label}</strong>
        <span>{additionalInfo ?? `${Math.round(value)}%`}</span>
      </div>
      <progress
        className={`progress ${status === "error" ? "progress-error" : "progress-primary"}`}
        value={Math.max(0, Math.min(100, value))}
        max="100"
        aria-label={typeof label === "string" ? label : undefined}
      />
      {description && <p className="ui-muted">{description}</p>}
    </div>
  );
}
export function CopyToClipboard({
  textToCopy,
  copyButtonAriaLabel = "コピー",
  copySuccessText = "コピーしました",
  copyErrorText = "コピーできませんでした",
}: Props) {
  const [status, setStatus] = useState("");
  return (
    <span className="ui-copy">
      <button
        type="button"
        className="btn btn-ghost btn-square btn-sm"
        aria-label={copyButtonAriaLabel}
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(textToCopy);
            setStatus(copySuccessText);
          } catch {
            setStatus(copyErrorText);
          }
        }}
      >
        <Copy size={16} />
      </button>
      <span className="sr-only" role="status">
        {status}
      </span>
    </span>
  );
}
export function Tabs({
  tabs = [],
  activeTabId,
  onChange,
}: Props & { onChange?: (event: Change<{ activeTabId: string }>) => void }) {
  const [own, setOwn] = useState(tabs[0]?.id),
    active = activeTabId ?? own,
    id = useId();
  const select = (next: string) => {
    setOwn(next);
    onChange?.(change({ activeTabId: next }));
  };
  return (
    <div>
      <div className="tabs tabs-border ui-tabs" role="tablist">
        {tabs.map((tab: any, index: number) => (
          <button
            type="button"
            role="tab"
            key={tab.id}
            id={`${id}-${tab.id}`}
            aria-selected={active === tab.id}
            aria-controls={`${id}-${tab.id}-panel`}
            tabIndex={active === tab.id ? 0 : -1}
            className={`tab ${active === tab.id ? "tab-active" : ""}`}
            onClick={() => select(tab.id)}
            onKeyDown={(event) => {
              if (event.key !== "ArrowRight" && event.key !== "ArrowLeft")
                return;
              event.preventDefault();
              const next =
                tabs[
                  (index +
                    (event.key === "ArrowRight" ? 1 : -1) +
                    tabs.length) %
                    tabs.length
                ];
              select(next.id);
              document.getElementById(`${id}-${next.id}`)?.focus();
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>
      {tabs.map((tab: any) =>
        active === tab.id ? (
          <div
            key={tab.id}
            role="tabpanel"
            id={`${id}-${tab.id}-panel`}
            aria-labelledby={`${id}-${tab.id}`}
            className="ui-tab-panel"
          >
            {tab.content}
          </div>
        ) : null,
      )}
    </div>
  );
}
export function AttributeEditor<T = any>({
  items = [],
  definition = [],
  onAddButtonClick,
  onRemoveButtonClick,
  addButtonText,
  removeButtonText,
  removeButtonAriaLabel,
  empty,
}: Props & {
  items?: T[];
  onRemoveButtonClick?: (event: Change<{ itemIndex: number; item: T }>) => void;
}) {
  return (
    <div className="ui-attribute-editor">
      {!items.length && empty}
      {items.map((item, index) => (
        <section className="ui-attribute-row" key={index}>
          <div className="ui-attribute-row__header">
            <strong>ルール {index + 1}</strong>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              aria-label={removeButtonAriaLabel?.(item) ?? removeButtonText}
              onClick={() =>
                onRemoveButtonClick?.(change({ itemIndex: index, item }))
              }
            >
              <Trash2 size={15} />
              {removeButtonText}
            </button>
          </div>
          <div className="ui-attribute-grid">
            {definition.map((field: any, i: number) => (
              <FormField key={i} label={field.label} description={field.info}>
                {field.control(item, index)}
              </FormField>
            ))}
          </div>
        </section>
      ))}
      <Button iconName="add-plus" onClick={onAddButtonClick}>
        {addButtonText ?? "追加"}
      </Button>
    </div>
  );
}

/** Filter, sort and paginate locally without changing the data passed to the table. */
export function useCollection<T>(source: T[], settings: Props) {
  const [filteringText, setFilteringText] = useState("");
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<{
    column: TableProps.ColumnDefinition<T>;
    desc: boolean;
  } | null>(null);
  const filtered =
    filteringText && settings.filtering?.filteringFunction
      ? source.filter((item) =>
          settings.filtering.filteringFunction(item, filteringText),
        )
      : source;
  const ordered = sort
    ? [...filtered].sort((a, b) => {
        const column = sort.column;
        const n = column.sortingComparator
          ? column.sortingComparator(a, b)
          : String(
              (a as any)[column.sortingField ?? column.id ?? ""] ?? "",
            ).localeCompare(
              String((b as any)[column.sortingField ?? column.id ?? ""] ?? ""),
              "ja",
              { numeric: true },
            );
        return sort.desc ? -n : n;
      })
    : filtered;
  const size = settings.pagination?.pageSize ?? Math.max(source.length, 1),
    pagesCount = Math.max(1, Math.ceil(ordered.length / size)),
    currentPageIndex = Math.min(page, pagesCount);
  return {
    items: ordered.slice(
      (currentPageIndex - 1) * size,
      currentPageIndex * size,
    ),
    filteredItemsCount: filtered.length,
    collectionProps: {
      empty: filteringText
        ? settings.filtering?.noMatch
        : settings.filtering?.empty,
      sortingColumn: sort?.column,
      sortingDescending: sort?.desc,
      onSortingChange: ({
        detail,
      }: {
        detail: {
          sortingColumn: TableProps.ColumnDefinition<T>;
          isDescending: boolean;
        };
      }) =>
        setSort({ column: detail.sortingColumn, desc: detail.isDescending }),
    },
    filterProps: {
      filteringText,
      onChange: ({ detail }: { detail: { filteringText: string } }) => {
        setFilteringText(detail.filteringText);
        setPage(1);
      },
    },
    paginationProps: {
      currentPageIndex,
      pagesCount,
      onChange: ({ detail }: { detail: { currentPageIndex: number } }) =>
        setPage(detail.currentPageIndex),
    },
  };
}

export function BarChart({
  series = [],
  height = 240,
  ariaLabel,
  yTitle,
  empty,
}: Props) {
  const points = series.flatMap((s: any) => s.data ?? []),
    max = Math.max(1, ...points.map((p: any) => Number(p.y) || 0));
  if (!points.length)
    return (
      <div className="ui-chart-empty">{empty ?? "データがありません"}</div>
    );
  return (
    <div
      className="ui-bar-chart"
      role="img"
      aria-label={ariaLabel ?? yTitle ?? "棒グラフ"}
      style={{ minHeight: height }}
    >
      {points.map((p: any, index: number) => (
        <div className="ui-bar-chart__item" key={index}>
          <span className="ui-bar-chart__value">{p.y}</span>
          <div className="ui-bar-chart__track">
            <div
              className="ui-bar-chart__bar"
              style={{
                height: `${Math.max(0, (Number(p.y) || 0) / max) * 100}%`,
              }}
            />
          </div>
          <span className="ui-bar-chart__label">{String(p.x)}</span>
        </div>
      ))}
    </div>
  );
}
export function LineChart({
  series = [],
  height = 220,
  ariaLabel,
  loadingText,
  statusType,
  empty,
  i18nStrings,
}: Props) {
  const points = series
    .flatMap((s: any) => s.data ?? [])
    .filter(
      (p: any) =>
        Number.isFinite(Number(p.y)) &&
        Number.isFinite(new Date(p.x).getTime()),
    );
  if (!points.length)
    return (
      <div className="ui-chart-empty" style={{ minHeight: height }}>
        {statusType === "loading"
          ? (loadingText ?? "読み込んでいます")
          : (empty ?? "データがありません")}
      </div>
    );
  const xs = points.map((p: any) => new Date(p.x).getTime()),
    ys = points.map((p: any) => Number(p.y)),
    xmin = Math.min(...xs),
    xmax = Math.max(...xs),
    ymin = Math.min(0, ...ys),
    ymax = Math.max(...ys, ymin + 1);
  const x = (p: any) =>
      24 + ((new Date(p.x).getTime() - xmin) / (xmax - xmin || 1)) * 752,
    y = (p: any) => 180 - ((Number(p.y) - ymin) / (ymax - ymin)) * 155;
  const formatY = (value: number) =>
    i18nStrings?.yTickFormatter
      ? i18nStrings.yTickFormatter(value)
      : String(Math.round(value * 100) / 100);
  return (
    <div className="ui-line-chart">
      <div className="ui-line-chart__plot">
        <div className="ui-line-chart__axis" aria-hidden="true">
          <span>{formatY(ymax)}</span>
          <span>{formatY((ymin + ymax) / 2)}</span>
          <span>{formatY(ymin)}</span>
        </div>
        <svg
          viewBox="0 0 800 200"
          width="100%"
          height={height}
          preserveAspectRatio="none"
          role="img"
          aria-label={ariaLabel ?? "推移"}
        >
          <line x1="24" y1="180" x2="776" y2="180" className="ui-chart-axis" />
          {series.map((s: any, index: number) => (
            <g key={s.title ?? index}>
              <polyline
                fill="none"
                className="ui-chart-line"
                points={(s.data ?? [])
                  .filter(
                    (p: any) =>
                      Number.isFinite(Number(p.y)) &&
                      Number.isFinite(new Date(p.x).getTime()),
                  )
                  .map((p: any) => `${x(p)},${y(p)}`)
                  .join(" ")}
              />
              {(s.data ?? [])
                .filter(
                  (p: any) =>
                    Number.isFinite(Number(p.y)) &&
                    Number.isFinite(new Date(p.x).getTime()),
                )
                .map((p: any, i: number) => (
                  <circle
                    key={i}
                    cx={x(p)}
                    cy={y(p)}
                    r="3"
                    tabIndex={0}
                    className="ui-chart-point"
                  >
                    <title>{`${s.title ?? ""} ${new Date(p.x).toLocaleString()}: ${p.y}`}</title>
                  </circle>
                ))}
            </g>
          ))}
        </svg>
      </div>
      <div className="ui-chart-range">
        <span>{new Date(xmin).toLocaleString()}</span>
        <span>{new Date(xmax).toLocaleString()}</span>
      </div>
    </div>
  );
}

/** A list of arbitrary values with visible, individually removable entries. */
export function TagInput({
  values = [],
  onChange,
  placeholder = "追加する値",
  ariaLabel = "値を追加",
  disabled = false,
}: {
  values: string[];
  onChange: (values: string[]) => void;
  placeholder?: string;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  const [draft, setDraft] = useState("");
  const field = useContext(Field);
  const add = () => {
    const entries = draft
      .split(/[,\n]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (!entries.length) return;
    onChange([...new Set([...values, ...entries])]);
    setDraft("");
  };
  return (
    <div
      className="ui-tag-input"
      onBlur={(event) => {
        if (
          !disabled &&
          !event.currentTarget.contains(event.relatedTarget as Node | null)
        )
          add();
      }}
    >
      <div className="ui-tag-input__entry">
        <input
          {...fieldAttributes(field, ariaLabel)}
          className="input ui-input"
          value={draft}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(e) => setDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <Button
          disabled={disabled || !draft.trim()}
          iconName="add-plus"
          onClick={add}
        >
          追加
        </Button>
      </div>
      {values.length > 0 && (
        <ul className="ui-tags">
          {values.map((value) => (
            <li className="badge badge-soft badge-primary" key={value}>
              {value}
              <button
                type="button"
                disabled={disabled}
                aria-label={`${value} を削除`}
                onClick={() => onChange(values.filter((v) => v !== value))}
              >
                <X size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
