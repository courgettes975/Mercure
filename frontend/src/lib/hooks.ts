import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";

// Valeur retardée : évite une requête à chaque frappe dans un champ de recherche.
export function useDebounced<T>(value: T, delay = 300): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

/** Filtres de liste conservés dans l'adresse (retour arrière et liens partageables). */
export function useUrlState<K extends string>(defaults: Record<K, string>) {
  const [params, setParams] = useSearchParams();
  const values = Object.fromEntries(
    (Object.keys(defaults) as K[]).map((k) => [k, params.get(k) ?? defaults[k]]),
  ) as Record<K, string>;
  // Les valeurs par défaut et vides ne sont pas écrites dans l'adresse (URL propre).
  const set = (patch: Partial<Record<K, string>>) => {
    const next = new URLSearchParams(params);
    for (const [k, v] of Object.entries(patch) as [K, string][]) {
      if (v === defaults[k] || v === "") next.delete(k);
      else next.set(k, v);
    }
    setParams(next, { replace: true });
  };
  return [values, set] as const;
}
