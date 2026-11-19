// Tableau générique : colonnes typées, tri local optionnel, clic sur ligne.

import { ReactNode, useMemo, useState } from "react";

// Définition d'une colonne : rendu, valeur de tri facultative, alignement.
export interface Column<T> {
  key: string;
  header: ReactNode;
  render: (row: T) => ReactNode;
  sortValue?: (row: T) => string | number;
  align?: "left" | "right" | "center";
  width?: string;
}

// Tableau générique ; tri local sur les colonnes qui fournissent `sortValue`, ou tri serveur si `serverSort`.
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  onRowClick,
  empty = "Aucun élément",
  serverSort,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string | number;
  onRowClick?: (row: T) => void;
  empty?: ReactNode;
  /** Tri côté serveur : clé courante ("name" ou "-name") et rappel. */
  serverSort?: { value: string; onChange: (v: string) => void; keys: string[] };
}) {
  // Tri courant : colonne + sens (1 croissant, -1 décroissant).
  const [localSort, setLocalSort] = useState<{ key: string; dir: 1 | -1 } | null>(null);
  const sort = serverSort
    ? { key: serverSort.value.replace(/^-/, ""), dir: (serverSort.value.startsWith("-") ? -1 : 1) as 1 | -1 }
    : localSort;

  const sorted = useMemo(() => {
    if (!sort || serverSort) return rows;
    const col = columns.find((c) => c.key === sort.key);
    if (!col?.sortValue) return rows;
    const get = col.sortValue;
    return [...rows].sort((a, b) => {
      const va = get(a);
      const vb = get(b);
      const cmp = typeof va === "number" && typeof vb === "number" ? va - vb : String(va).localeCompare(String(vb), "fr", { sensitivity: "base" });
      return cmp * sort.dir;
    });
  }, [rows, sort, columns, serverSort]);

  const toggle = (key: string) => {
    if (serverSort) {
      serverSort.onChange(sort?.key === key && sort.dir === 1 ? `-${key}` : key);
      return;
    }
    setLocalSort((s) => (s?.key === key ? (s.dir === 1 ? { key, dir: -1 } : null) : { key, dir: 1 }));
  };
  const sortable = (c: Column<T>) => (serverSort ? serverSort.keys.includes(c.key) : !!c.sortValue);

  return (
    <div className="table-wrap">
      <table className="table">
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} style={{ width: c.width, textAlign: c.align }} aria-sort={sort?.key === c.key ? (sort.dir === 1 ? "ascending" : "descending") : undefined}>
                {sortable(c) ? (
                  <button className="th-sort" onClick={() => toggle(c.key)}>
                    {c.header}
                    <span className="sort-ind">{sort?.key === c.key ? (sort.dir === 1 ? " ▲" : " ▼") : ""}</span>
                  </button>
                ) : (
                  c.header
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.length === 0 ? (
            <tr>
              <td colSpan={columns.length} className="table-empty">
                {empty}
              </td>
            </tr>
          ) : (
            sorted.map((row) => (
              <tr
                key={rowKey(row)}
                className={onRowClick ? "clickable" : undefined}
                onClick={onRowClick ? () => onRowClick(row) : undefined}
                tabIndex={onRowClick ? 0 : undefined}
                onKeyDown={onRowClick ? (e) => e.key === "Enter" && onRowClick(row) : undefined}
              >
                {columns.map((c) => (
                  <td key={c.key} style={{ textAlign: c.align }}>
                    {c.render(row)}
                  </td>
                ))}
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
