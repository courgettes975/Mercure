// Bibliothèque de composants communs. Un seul composant par usage : à réutiliser partout.

import {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
  useEffect,
  useId,
  useRef,
} from "react";

// Styles de bouton disponibles.
type Variant = "primary" | "secondary" | "danger" | "ghost";

// Bouton avec indicateur de chargement (désactivé pendant l'action).
export function Button({
  variant = "secondary",
  loading,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; loading?: boolean }) {
  return (
    <button {...rest} className={`btn btn-${variant} ${rest.className ?? ""}`} disabled={rest.disabled || loading}>
      {loading ? <span className="spinner" aria-hidden /> : null}
      {children}
    </button>
  );
}

// Enveloppe commune des champs : libellé, message d'erreur ou d'aide, astérisque obligatoire.
interface FieldProps {
  label: string;
  error?: string;
  hint?: string;
  required?: boolean;
  children: (id: string) => ReactNode;
  wide?: boolean;
}

// Relie le libellé au contrôle par un id unique (accessibilité).
export function Field({ label, error, hint, required, children, wide }: FieldProps) {
  const id = useId();
  return (
    <div className={`field ${wide ? "field-wide" : ""} ${error ? "field-error" : ""}`}>
      <label htmlFor={id}>
        {label}
        {required ? <span className="required" aria-label="obligatoire"> *</span> : null}
      </label>
      {children(id)}
      {error ? <div className="field-msg error" role="alert">{error}</div> : hint ? <div className="field-msg">{hint}</div> : null}
    </div>
  );
}

// Champ texte / nombre / date standard.
export function TextField({
  label,
  error,
  hint,
  wide,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label: string; error?: string; hint?: string; wide?: boolean }) {
  return (
    <Field label={label} error={error} hint={hint} required={rest.required} wide={wide}>
      {(id) => <input id={id} {...rest} aria-invalid={!!error} />}
    </Field>
  );
}

// Zone de texte multi-lignes (toujours sur toute la largeur).
export function TextAreaField({
  label,
  error,
  hint,
  ...rest
}: TextareaHTMLAttributes<HTMLTextAreaElement> & { label: string; error?: string; hint?: string }) {
  return (
    <Field label={label} error={error} hint={hint} required={rest.required} wide>
      {(id) => <textarea id={id} rows={3} {...rest} aria-invalid={!!error} />}
    </Field>
  );
}

// Liste déroulante alimentée par un tableau {value, label}.
export function SelectField({
  label,
  error,
  hint,
  options,
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement> & {
  label: string;
  error?: string;
  hint?: string;
  options: { value: string; label: string }[];
}) {
  return (
    <Field label={label} error={error} hint={hint} required={rest.required}>
      {(id) => (
        <select id={id} {...rest}>
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      )}
    </Field>
  );
}

// Case à cocher avec son libellé cliquable.
export function Checkbox({ label, ...rest }: InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  return (
    <label className="checkbox">
      <input type="checkbox" {...rest} />
      {label}
    </label>
  );
}

// Couleurs sémantiques des pastilles et alertes.
export type BadgeTone = "neutral" | "success" | "warning" | "danger" | "info";

// Pastille d'état.
export function Badge({ tone = "neutral", children }: { tone?: BadgeTone; children: ReactNode }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

// Message d'alerte ; rien n'est rendu si le contenu est vide.
export function Alert({ tone = "danger", children }: { tone?: BadgeTone; children: ReactNode }) {
  if (!children) return null;
  return (
    <div className={`alert alert-${tone}`} role={tone === "danger" ? "alert" : "status"}>
      {children}
    </div>
  );
}

// Fenêtre modale basée sur <dialog> natif (focus et touche Échap gérés par le navigateur).
export function Modal({
  title,
  open,
  onClose,
  children,
  footer,
}: {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  // Synchronise l'état React `open` avec l'élément <dialog>.
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog ref={ref} className="modal" onClose={onClose} aria-labelledby="modal-title">
      <div className="modal-head">
        <h2 id="modal-title">{title}</h2>
        <button className="icon-btn" onClick={onClose} aria-label="Fermer">
          ×
        </button>
      </div>
      <div className="modal-body">{open ? children : null}</div>
      {footer ? <div className="modal-foot">{footer}</div> : null}
    </dialog>
  );
}

// Demande de confirmation (titre, message, bouton confirmer éventuellement en rouge).
export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = "Confirmer",
  danger,
  loading,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message: ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      open={open}
      title={title}
      onClose={onCancel}
      footer={
        <>
          <Button onClick={onCancel}>Annuler</Button>
          <Button variant={danger ? "danger" : "primary"} loading={loading} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p>{message}</p>
    </Modal>
  );
}

// Champ de recherche avec icône et bouton d'effacement.
export function SearchInput({
  value,
  onChange,
  placeholder = "Rechercher...",
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  return (
    <div className="search">
      <span className="search-icon" aria-hidden>
        ⌕
      </span>
      <input type="search" value={value} placeholder={placeholder} aria-label={placeholder} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}

// Pagination "Précédent / page x sur n / Suivant".
export function Pagination({
  page,
  perPage,
  total,
  onPage,
}: {
  page: number;
  perPage: number;
  total: number;
  onPage: (p: number) => void;
}) {
  const from = total === 0 ? 0 : (page - 1) * perPage + 1;
  const to = Math.min(page * perPage, total);
  const last = Math.max(1, Math.ceil(total / perPage));
  return (
    <div className="pagination">
      <span>
        {from}-{to} / {total}
      </span>
      <button className="icon-btn" disabled={page <= 1} onClick={() => onPage(page - 1)} aria-label="Page précédente">
        ‹
      </button>
      <button className="icon-btn" disabled={page >= last} onClick={() => onPage(page + 1)} aria-label="Page suivante">
        ›
      </button>
    </div>
  );
}

// En-tête de page : titre, actions à droite, contenu complémentaire (filtres).
export function PageHeader({ title, actions, children }: { title: string; actions?: ReactNode; children?: ReactNode }) {
  return (
    <div className="page-header">
      <div className="page-header-left">
        {actions}
        <h1>{title}</h1>
      </div>
      <div className="page-header-right">{children}</div>
    </div>
  );
}

// Bloc de contenu encadré, avec titre et actions facultatifs.
export function Card({ title, children, actions }: { title?: ReactNode; children: ReactNode; actions?: ReactNode }) {
  return (
    <section className="card">
      {title || actions ? (
        <div className="card-head">
          {title ? <h2>{title}</h2> : <span />}
          {actions}
        </div>
      ) : null}
      {children}
    </section>
  );
}

// Message centré quand une liste est vide.
export function EmptyState({ children }: { children: ReactNode }) {
  return <div className="empty">{children}</div>;
}

// Indicateur de chargement.
export function Loading() {
  return (
    <div className="loading" role="status">
      <span className="spinner" /> Chargement...
    </div>
  );
}
