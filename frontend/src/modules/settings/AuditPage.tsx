import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../api/client";
import type { AuditEntry, Page } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { Card, Loading, Pagination, SelectField } from "../../components/ui";
import { formatDateTime } from "../../lib/format";

const ENTITY_LABELS: Record<string, string> = {
  user: "Utilisateur",
  company_settings: "Paramètres entreprise",
  software: "Logiciel",
  backup: "Sauvegarde",
  contact: "Contact",
  tag: "Étiquette",
  project: "Projet",
  mission: "Mission",
  timesheet: "Temps",
  invoice: "Facture",
  demo: "Démonstration",
  storage: "Stockage",
  network: "Réseau",
  training_center: "Centre de formation",
  product: "Produit",
  expense: "Dépense",
};

// Rend le JSON des changements lisible (diff avant/après ou valeurs).
function formatChanges(raw: string | null): string {
  if (!raw) return "";
  try {
    const obj = JSON.parse(raw) as Record<string, unknown>;
    return Object.entries(obj)
      .map(([k, v]) => {
        if (v && typeof v === "object" && "old" in v && "new" in v) {
          const c = v as { old: unknown; new: unknown };
          return `${k} : ${JSON.stringify(c.old)} -> ${JSON.stringify(c.new)}`;
        }
        return `${k} : ${JSON.stringify(v)}`;
      })
      .join(" | ");
  } catch {
    return raw;
  }
}

// Journal d'audit paginé, filtrable par type d'entité.
export function AuditPage() {
  const [page, setPage] = useState(1);
  const [entity, setEntity] = useState("");
  const { data, isLoading } = useQuery({
    queryKey: ["audit", page, entity],
    queryFn: () => api.get<Page<AuditEntry>>(`/audit?page=${page}&per_page=50${entity ? `&entity_type=${entity}` : ""}`),
  });

  const columns: Column<AuditEntry>[] = [
    { key: "date", header: "Date", width: "150px", render: (a) => formatDateTime(a.created_at) },
    { key: "user", header: "Utilisateur", width: "160px", render: (a) => a.user_name ?? "Système" },
    { key: "action", header: "Action", width: "180px", render: (a) => a.action },
    { key: "obj", header: "Objet", width: "200px", render: (a) => `${ENTITY_LABELS[a.entity_type] ?? a.entity_type}${a.entity_id ? ` #${a.entity_id}` : ""}` },
    { key: "changes", header: "Détail", render: (a) => <span className="small">{formatChanges(a.changes)}</span> },
  ];

  return (
    <Card
      title="Journal des opérations"
      actions={
        <div className="toolbar">
          <SelectField
            label="Objet"
            value={entity}
            onChange={(e) => {
              setEntity(e.target.value);
              setPage(1);
            }}
            options={[{ value: "", label: "Tous" }, ...Object.entries(ENTITY_LABELS).map(([value, label]) => ({ value, label }))]}
          />
          {data ? <Pagination page={data.page} perPage={data.per_page} total={data.total} onPage={setPage} /> : null}
        </div>
      }
    >
      {isLoading || !data ? <Loading /> : <DataTable columns={columns} rows={data.items} rowKey={(a) => a.id} empty="Aucune opération enregistrée" />}
    </Card>
  );
}
