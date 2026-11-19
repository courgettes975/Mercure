import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, ApiError } from "../../api/client";
import { KIND_LABELS, type ContactInput } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { useToast } from "../../components/Toast";
import { Alert, Badge, Button, Card } from "../../components/ui";
import { RoleBadges } from "./common";

// Contact planifié tel que renvoyé par l'aperçu serveur.
interface PlannedContact {
  import_ref: string;
  source: string;
  input: ContactInput;
  parent_name: string | null;
  tags: string[];
  remarks: string[];
  already_imported: boolean;
}

// Aperçu : détail + compteurs.
interface ImportPlan {
  items: PlannedContact[];
  companies: number;
  persons: number;
  duplicates: number;
  already_imported: number;
}

// Filtres d'affichage de l'aperçu.
type Filter = "tous" | "doublons" | "a-completer" | "stagiaires";

// Import Odoo : choix du fichier -> aperçu (rien n'est écrit) -> import en une transaction.
export function ImportPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const toast = useToast();
  const [file, setFile] = useState<File | null>(null);
  const [plan, setPlan] = useState<ImportPlan | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState<Filter>("tous");

  // Envoi du fichier en multipart.
  const send = async <T,>(url: string, f: File) => {
    const fd = new FormData();
    fd.append("file", f);
    return api.post<T>(url, fd);
  };

  // Demande l'aperçu au serveur.
  const preview = async (f: File) => {
    setFile(f);
    setError("");
    setPlan(null);
    setBusy(true);
    try {
      setPlan(await send<ImportPlan>("/contacts/import/preview", f));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    } finally {
      setBusy(false);
    }
  };

  // Exécute l'import ; le serveur relit le fichier et refait le plan (pas de confiance au navigateur).
  const run = async () => {
    if (!file) return;
    setBusy(true);
    try {
      const r = await send<{ created: number; skipped: number }>("/contacts/import", file);
      qc.invalidateQueries({ queryKey: ["contacts"] });
      qc.invalidateQueries({ queryKey: ["tags"] });
      toast(`${r.created} contact(s) importé(s)${r.skipped ? `, ${r.skipped} déjà présent(s)` : ""}`);
      nav("/contacts");
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Erreur inattendue");
    } finally {
      setBusy(false);
    }
  };

  // Lignes affichées selon le filtre choisi.
  const rows = (plan?.items ?? []).filter((p) => {
    if (filter === "doublons") return p.tags.includes("Doublon possible");
    if (filter === "a-completer") return p.tags.includes("À compléter") || p.tags.includes("Fiche formation Odoo");
    if (filter === "stagiaires") return p.input.is_stagiaire;
    return true;
  });
  const toCreate = (plan?.items ?? []).filter((p) => !p.already_imported).length;

  const columns: Column<PlannedContact>[] = [
    { key: "src", header: "Nom dans Odoo", render: (p) => p.source, sortValue: (p) => p.source },
    { key: "kind", header: "Type", render: (p) => KIND_LABELS[p.input.kind], sortValue: (p) => p.input.kind },
    { key: "parent", header: "Société", render: (p) => p.parent_name ?? "" },
    { key: "name", header: "Nom / Prénom", render: (p) => `${p.input.name}${p.input.first_name ? ` / ${p.input.first_name}` : ""}` },
    { key: "email", header: "Email", render: (p) => p.input.email },
    { key: "tel", header: "Téléphone", render: (p) => [p.input.phone, p.input.mobile].filter(Boolean).join(" / ") },
    { key: "roles", header: "Rôle", render: (p) => <RoleBadges c={p.input} /> },
    {
      key: "remarks",
      header: "Remarques",
      render: (p) => (
        <span className="small">
          {p.already_imported ? <Badge>Déjà importé</Badge> : null} {p.remarks.join(" ; ")}
        </span>
      ),
    },
  ];

  return (
    <>
      <div className="page-header">
        <div className="page-header-left">
          <div className="breadcrumb">
            <Link to="/contacts">Contacts</Link>
            <span>Importer</span>
          </div>
        </div>
      </div>
      <div className="page-body wide">
        <Card title="Importer des contacts (export Odoo)">
          <p className="muted">
            Fichier accepté : export Odoo des contacts en .xlsx ou .csv (colonnes « Nom d'affichage », « E-mail », « Téléphone », « Pays »... ; adresse, SIRET, TVA,
            étiquettes et « Est une société » sont aussi reconnus s'ils sont présents). Un aperçu est affiché avant tout enregistrement. Un même fichier peut être
            réimporté sans créer de doublon.
          </p>
          <div className="toolbar">
            <label className="btn btn-secondary">
              {file ? `Fichier : ${file.name}` : "Choisir un fichier"}
              <input type="file" hidden accept=".xlsx,.xls,.ods,.csv" onChange={(e) => e.target.files?.[0] && preview(e.target.files[0])} />
            </label>
            {plan ? (
              <Button variant="primary" loading={busy} disabled={toCreate === 0} onClick={run}>
                Importer {toCreate} contact(s)
              </Button>
            ) : null}
          </div>
          <Alert>{error}</Alert>
        </Card>
        {plan ? (
          <Card
            title={`Aperçu : ${plan.companies} société(s), ${plan.persons} personne(s)`}
            actions={
              <div className="segmented small-seg">
                {(
                  [
                    ["tous", `Tous (${plan.items.length})`],
                    ["doublons", `Doublons possibles (${plan.duplicates})`],
                    ["a-completer", "À compléter"],
                    ["stagiaires", "Stagiaires"],
                  ] as [Filter, string][]
                ).map(([k, l]) => (
                  <label key={k} className={filter === k ? "active" : ""}>
                    <input type="radio" name="f" checked={filter === k} onChange={() => setFilter(k)} />
                    {l}
                  </label>
                ))}
              </div>
            }
          >
            <Alert tone="info">
              Chaque contact importé reçoit l'étiquette « Import Odoo ». Les doublons possibles, les contacts à compléter et les fiches formation Odoo sont étiquetés pour être
              revus ensuite depuis la liste des contacts (filtre par étiquette). Rien n'est fusionné ni supprimé automatiquement.
              {plan.already_imported ? ` ${plan.already_imported} contact(s) déjà importé(s) seront ignorés.` : ""}
            </Alert>
            <DataTable columns={columns} rows={rows} rowKey={(p) => p.import_ref} />
          </Card>
        ) : null}
      </div>
    </>
  );
}
