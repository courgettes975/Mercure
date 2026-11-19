import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useState } from "react";
import { api, ApiError } from "../../api/client";
import type { TrainingCenter } from "../../api/types";
import { Column, DataTable } from "../../components/DataTable";
import { AsyncSelect, Option } from "../../components/Pickers";
import { Button, Card, ConfirmDialog, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";
import { searchContacts } from "../../lib/search";
import { Link } from "react-router";

// Liste des centres de formation : ajout d'un contact existant ou création d'un organisme, retrait.
export function TrainingCentersPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: ["training-centers"], queryFn: () => api.get<TrainingCenter[]>("/training-centers") });
  const [existing, setExisting] = useState<Option | null>(null);
  const [name, setName] = useState("");
  const [toRemove, setToRemove] = useState<TrainingCenter | null>(null);
  const onError = (e: unknown) => toast(e instanceof ApiError ? e.message : "Erreur inattendue", "danger");
  const refresh = () => qc.invalidateQueries({ queryKey: ["training-centers"] });

  // Ajout (partner_id OU name).
  const add = useMutation({
    mutationFn: (body: { partner_id?: number; name?: string }) => api.post<TrainingCenter>("/training-centers", body),
    onSuccess: () => { setExisting(null); setName(""); refresh(); toast("Centre ajouté"); },
    onError,
  });
  // Retrait de la liste (le contact et les missions restent).
  const remove = useMutation({
    mutationFn: (id: number) => api.del(`/training-centers/${id}`),
    onSuccess: () => { setToRemove(null); refresh(); toast("Centre retiré de la liste"); },
    onError,
  });

  const columns: Column<TrainingCenter>[] = [
    { key: "name", header: "Centre de formation", render: (c) => <Link to={`/contacts/${c.id}`}>{c.name}</Link>, sortValue: (c) => c.name },
    { key: "kind", header: "Type", render: (c) => (c.kind === "ORGANISME" ? "Organisme" : "Entreprise") },
    { key: "city", header: "Ville", render: (c) => c.city },
    { key: "missions", header: "Missions", align: "right", render: (c) => c.missions_count, sortValue: (c) => c.missions_count },
    { key: "actions", header: "", align: "right", render: (c) => <Button variant="ghost" onClick={() => setToRemove(c)}>Retirer</Button> },
  ];

  const submitNew = (e: FormEvent) => {
    e.preventDefault();
    if (name.trim()) add.mutate({ name });
  };

  return (
    <Card title="Centres de formation">
      <p className="muted">
        Liste proposée dans le champ "Centre de formation" des missions. Chaque centre est un contact (société ou organisme) : la fiche
        contact reste la référence pour l'adresse et la facturation.
      </p>
      <div className="inline-form">
        <AsyncSelect label="Ajouter un contact existant" value={existing} search={(q) => searchContacts(q, { companiesOnly: true })} onChange={setExisting} />
        <Button variant="primary" disabled={!existing} loading={add.isPending} onClick={() => existing && add.mutate({ partner_id: existing.id })}>Ajouter</Button>
      </div>
      <form onSubmit={submitNew} className="inline-form">
        <TextField label="Ou créer un nouvel organisme" value={name} onChange={(e) => setName(e.target.value)} placeholder="ex. BT Formation" />
        <Button type="submit" loading={add.isPending}>Créer et ajouter</Button>
      </form>
      <DataTable columns={columns} rows={data ?? []} rowKey={(c) => c.id} empty="Aucun centre pour l'instant." />
      <ConfirmDialog open={!!toRemove} title="Retirer de la liste" onCancel={() => setToRemove(null)} onConfirm={() => toRemove && remove.mutate(toRemove.id)}
        message={toRemove ? `${toRemove.name} ne sera plus proposé dans les nouvelles missions. Le contact et les ${toRemove.missions_count} mission(s) liées sont conservés.` : ""} />
    </Card>
  );
}
