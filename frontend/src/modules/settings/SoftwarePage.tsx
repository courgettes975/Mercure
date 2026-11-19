import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useState } from "react";
import { api, ApiError } from "../../api/client";
import type { Software } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { Badge, Button, Card, Loading, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";

// Liste des logiciels enseignés : ajout, archivage / réactivation.
export function SoftwarePage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({ queryKey: ["software"], queryFn: () => api.get<Software[]>("/software") });
  const [name, setName] = useState("");
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");

  const create = useMutation({
    mutationFn: (n: string) => api.post<Software>("/software", { name: n }),
    onSuccess: () => {
      setName("");
      qc.invalidateQueries({ queryKey: ["software"] });
      toast("Logiciel ajouté");
    },
    onError,
  });

  const toggle = useMutation({
    mutationFn: (s: Software) => api.patch<Software>(`/software/${s.id}`, { active: !s.active }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["software"] }),
    onError,
  });

  if (isLoading) return <Loading />;

  const columns: Column<Software>[] = [
    { key: "name", header: "Logiciel", render: (s) => s.name, sortValue: (s) => s.name },
    { key: "active", header: "Statut", render: (s) => (s.active ? <Badge tone="success">Actif</Badge> : <Badge>Archivé</Badge>) },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (s) => (
        <Button variant="ghost" onClick={() => toggle.mutate(s)}>
          {s.active ? "Archiver" : "Réactiver"}
        </Button>
      ),
    },
  ];

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (name.trim()) create.mutate(name);
  };

  return (
    <Card title="Logiciels enseignés">
      <p className="muted">Liste utilisée dans les missions de formation. Un logiciel archivé n'est plus proposé mais reste visible dans l'historique.</p>
      <form onSubmit={submit} className="inline-form">
        <TextField label="Nouveau logiciel" value={name} onChange={(e) => setName(e.target.value)} placeholder="ex. Inventor" />
        <Button variant="primary" type="submit" loading={create.isPending}>
          Ajouter
        </Button>
      </form>
      <DataTable columns={columns} rows={data ?? []} rowKey={(s) => s.id} />
    </Card>
  );
}
