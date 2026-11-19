// Sélecteurs avec recherche : enregistrement lié (AsyncSelect) et étiquettes (TagPicker).

import { useEffect, useId, useRef, useState } from "react";
import { useDebounced } from "../lib/hooks";

// Élément sélectionnable : identifiant + libellé affiché.
export interface Option {
  id: number;
  label: string;
}

// Champ de recherche avec suggestions chargées depuis l'API (clients, contacts, stagiaires).
export function AsyncSelect({
  label,
  value,
  onChange,
  search,
  placeholder = "Rechercher...",
  disabled,
}: {
  label: string;
  value: Option | null;
  onChange: (v: Option | null) => void;
  search: (q: string) => Promise<Option[]>;
  placeholder?: string;
  disabled?: boolean;
}) {
  const id = useId();
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [options, setOptions] = useState<Option[]>([]);
  const [active, setActive] = useState(0);
  const q = useDebounced(text, 200);
  const boxRef = useRef<HTMLDivElement>(null);

  // Recherche (différée de 200 ms) à chaque frappe tant que la liste est ouverte ; `alive` ignore les réponses obsolètes.
  useEffect(() => {
    if (!open) return;
    let alive = true;
    search(q).then((o) => alive && (setOptions(o), setActive(0))).catch(() => alive && setOptions([]));
    return () => {
      alive = false;
    };
  }, [q, open, search]);

  useEffect(() => {
    // Clic en dehors : fermeture de la liste.
    const close = (e: MouseEvent) => !boxRef.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  // Sélection d'une suggestion.
  const pick = (o: Option) => {
    onChange(o);
    setOpen(false);
    setText("");
  };

  return (
    <div className="field" ref={boxRef}>
      <label htmlFor={id}>{label}</label>
      <div className="combo">
        <input
          id={id}
          role="combobox"
          aria-expanded={open}
          aria-controls={`${id}-list`}
          disabled={disabled}
          value={open ? text : value?.label ?? ""}
          placeholder={placeholder}
          onFocus={() => setOpen(true)}
          onChange={(e) => {
            setText(e.target.value);
            setOpen(true);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, options.length - 1));
            else if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
            else if (e.key === "Enter" && open && options[active]) {
              e.preventDefault();
              pick(options[active]);
            } else if (e.key === "Escape") setOpen(false);
          }}
        />
        {value && !disabled ? (
          <button type="button" className="combo-clear" aria-label="Retirer" onClick={() => onChange(null)}>
            ×
          </button>
        ) : null}
        {open && !disabled ? (
          <ul className="combo-list" id={`${id}-list`} role="listbox">
            {options.length === 0 ? <li className="combo-empty">Aucun résultat</li> : null}
            {options.map((o, i) => (
              <li
                key={o.id}
                role="option"
                aria-selected={i === active}
                className={i === active ? "active" : ""}
                onMouseDown={(e) => {
                  e.preventDefault();
                  pick(o);
                }}
              >
                {o.label}
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </div>
  );
}

// Sélecteur d'étiquettes multiples avec création à la volée.
export function TagPicker({
  label,
  all,
  selected,
  onChange,
  onCreate,
  disabled,
}: {
  label: string;
  all: Option[];
  selected: number[];
  onChange: (ids: number[]) => void;
  onCreate?: (name: string) => Promise<Option>;
  disabled?: boolean;
}) {
  const id = useId();
  const [text, setText] = useState("");
  const chosen = all.filter((t) => selected.includes(t.id));
  const t = text.trim().toLowerCase();
  const suggestions = t ? all.filter((o) => !selected.includes(o.id) && o.label.toLowerCase().includes(t)).slice(0, 8) : [];
  const exact = all.some((o) => o.label.toLowerCase() === t);

  const add = async () => {
    if (!t) return;
    const match = all.find((o) => o.label.toLowerCase() === t);
    if (match) onChange([...selected, match.id]);
    else if (onCreate) onChange([...selected, (await onCreate(text.trim())).id]);
    setText("");
  };

  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <div className="tag-box">
        {chosen.map((c) => (
          <span key={c.id} className="chip">
            {c.label}
            {!disabled ? (
              <button type="button" aria-label={`Retirer ${c.label}`} onClick={() => onChange(selected.filter((s) => s !== c.id))}>
                ×
              </button>
            ) : null}
          </span>
        ))}
        {!disabled ? (
          <input
            id={id}
            value={text}
            placeholder={chosen.length ? "" : "ex. B2B, Conseil..."}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              }
            }}
          />
        ) : null}
      </div>
      {suggestions.length || (t && !exact && onCreate) ? (
        <div className="tag-suggest">
          {suggestions.map((s) => (
            <button type="button" key={s.id} className="chip chip-btn" onClick={() => (onChange([...selected, s.id]), setText(""))}>
              {s.label}
            </button>
          ))}
          {t && !exact && onCreate ? (
            <button type="button" className="chip chip-btn chip-new" onClick={add}>
              Créer « {text.trim()} »
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
