// Champs métier réutilisables : montant (centimes), durée (minutes), date, onglets, indicateurs.

import { ReactNode, useEffect, useState } from "react";
import { durationInput, formatMinutes, formatMoney, moneyInput, parseDuration, parseMoney } from "../lib/format";
import { Badge, BadgeTone, Field } from "./ui";

// Saisie d'un montant : texte libre ("80,50") converti en centimes à la sortie du champ.
export function MoneyField({
  label,
  value,
  onChange,
  disabled,
  hint,
}: {
  label: string;
  value: number;
  onChange: (cents: number) => void;
  disabled?: boolean;
  hint?: string;
}) {
  // Le texte affiché est distinct de la valeur : on ne convertit qu'au blur pour ne pas gêner la frappe.
  const [text, setText] = useState(moneyInput(value));
  const [error, setError] = useState("");
  useEffect(() => setText(moneyInput(value)), [value]);
  return (
    <Field label={label} error={error} hint={hint}>
      {(id) => (
        <div className="input-suffix">
          <input
            id={id}
            inputMode="decimal"
            disabled={disabled}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={() => {
              const v = parseMoney(text);
              if (v === null || v < 0) setError("Montant invalide (ex. 80 ou 80,50)");
              else {
                setError("");
                onChange(v);
                setText(moneyInput(v));
              }
            }}
          />
          <span>€</span>
        </div>
      )}
    </Field>
  );
}

// Saisie d'une durée : "7", "7h30", "45min" -> minutes à la sortie du champ.
export function DurationField({
  label,
  value,
  onChange,
  disabled,
}: {
  label: string;
  value: number;
  onChange: (minutes: number) => void;
  disabled?: boolean;
}) {
  const [text, setText] = useState(durationInput(value));
  const [error, setError] = useState("");
  useEffect(() => setText(durationInput(value)), [value]);
  return (
    <Field label={label} error={error} hint={!error && value ? formatMinutes(value) : undefined}>
      {(id) => (
        <input
          id={id}
          disabled={disabled}
          value={text}
          placeholder="ex. 7 ou 7h30"
          onChange={(e) => setText(e.target.value)}
          onBlur={() => {
            const v = parseDuration(text);
            if (v === null) setError("Durée invalide (ex. 7, 7,5 ou 7h30)");
            else {
              setError("");
              onChange(v);
            }
          }}
        />
      )}
    </Field>
  );
}

// Pastille d'un statut d'après une table {code -> libellé, couleur}.
export function StatusBadge<K extends string>({ map, value }: { map: Record<K, { label: string; tone: BadgeTone }>; value: K }) {
  const s = map[value];
  return s ? <Badge tone={s.tone}>{s.label}</Badge> : <Badge>{value}</Badge>;
}

// Barre d'onglets.
export function Tabs<K extends string>({ tabs, value, onChange }: { tabs: [K, ReactNode][]; value: K; onChange: (k: K) => void }) {
  return (
    <nav className="tabs inner" role="tablist">
      {tabs.map(([k, label]) => (
        <button key={k} type="button" role="tab" aria-selected={value === k} className={`tab ${value === k ? "active" : ""}`} onClick={() => onChange(k)}>
          {label}
        </button>
      ))}
    </nav>
  );
}

// Indicateur chiffré (tableau de bord) ; cliquable si `onClick`.
export function Stat({ label, value, sub, tone, onClick }: { label: string; value: ReactNode; sub?: ReactNode; tone?: "warning" | "danger" | "success"; onClick?: () => void }) {
  return (
    <button type="button" className={`stat ${tone ? `stat-${tone}` : ""} ${onClick ? "clickable" : ""}`} onClick={onClick} disabled={!onClick}>
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
      {sub ? <span className="stat-sub">{sub}</span> : null}
    </button>
  );
}

// Alias courts utilisés dans les tableaux.
export const money = formatMoney;
export const hours = formatMinutes;
