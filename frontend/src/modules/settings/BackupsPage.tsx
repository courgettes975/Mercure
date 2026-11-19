import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../../api/client";
import type { BackupFile } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { Badge, Button, Card, Loading } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { formatBytes } from "../../lib/format";

/** "auto-20261118-101500.db" -> "18/11/2026 10:15:00" */
// "auto-20261118-101500.db" -> "18/11/2026 10:15".
function dateFromName(name: string): string {
  const m = name.match(/(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/);
  return m ? `${m[3]}/${m[2]}/${m[1]} ${m[4]}:${m[5]}:${m[6]}` : name;
}

// Sauvegardes : liste, création manuelle, téléchargement, rappel du dossier.
export function BackupsPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({
    queryKey: ["backups"],
    queryFn: () => api.get<{ folder: string; files: BackupFile[] }>("/backups"),
  });

  const create = useMutation({
    mutationFn: () => api.post<{ name: string }>("/backups"),
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: ["backups"] });
      toast(`Sauvegarde créée : ${r.name}`);
    },
    onError: (e) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger"),
  });

  if (isLoading) return <Loading />;

  const columns: Column<BackupFile>[] = [
    { key: "date", header: "Date", render: (f) => dateFromName(f.name), sortValue: (f) => f.name.slice(f.name.indexOf("-")) },
    { key: "kind", header: "Type", render: (f) => (f.kind === "auto" ? <Badge tone="info">Automatique</Badge> : <Badge>Manuelle</Badge>) },
    { key: "size", header: "Taille", align: "right", render: (f) => formatBytes(f.size) },
    {
      key: "dl",
      header: "",
      align: "right",
      render: (f) => (
        <a className="btn btn-ghost" href={`/api/backups/${encodeURIComponent(f.name)}`} download>
          Télécharger
        </a>
      ),
    },
  ];

  return (
    <Card
      title="Sauvegardes"
      actions={
        <Button variant="primary" loading={create.isPending} onClick={() => create.mutate()}>
          Sauvegarder maintenant
        </Button>
      }
    >
      <p className="muted">
        Une sauvegarde automatique est faite chaque jour (les 30 dernières sont conservées). Dossier : <code>{data?.folder}</code>
        <br />
        Conseil : copiez régulièrement une sauvegarde sur un autre support (clé USB, cloud). Pour restaurer, arrêtez Mercure puis remplacez le fichier <code>erp.db</code> par la sauvegarde choisie.
      </p>
      <DataTable columns={columns} rows={data?.files ?? []} rowKey={(f) => f.name} empty="Aucune sauvegarde pour l'instant" />
    </Card>
  );
}
