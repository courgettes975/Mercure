import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormEvent, useState } from "react";
import { api, ApiError } from "../../api/client";
import { ROLE_LABELS, type Role, type UserRow } from "../../api/types";
import { useAuth } from "../../auth/AuthContext";
import { Column, DataTable } from "../../components/DataTable";
import { Alert, Badge, Button, Card, Checkbox, Loading, Modal, SearchInput, SelectField, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";

// Options du sélecteur de rôle.
const ROLE_OPTIONS = (Object.keys(ROLE_LABELS) as Role[]).map((r) => ({ value: r, label: ROLE_LABELS[r] }));

interface EditState {
  id?: number;
  email: string;
  display_name: string;
  role: Role;
  active: boolean;
  password: string;
}

// Gestion des comptes (ADMIN) : création, rôle, activation, réinitialisation du mot de passe.
export function UsersPage() {
  const { user: me } = useAuth();
  const qc = useQueryClient();
  const toast = useToast();
  const { data, isLoading } = useQuery({ queryKey: ["users"], queryFn: () => api.get<UserRow[]>("/users") });
  const [search, setSearch] = useState("");
  const [edit, setEdit] = useState<EditState | null>(null);
  const [error, setError] = useState("");

  // Création (POST) ou modification partielle (PATCH).
  const save = useMutation({
    mutationFn: (e: EditState) =>
      e.id
        ? api.patch<UserRow>(`/users/${e.id}`, { display_name: e.display_name, role: e.role, active: e.active, new_password: e.password || undefined })
        : api.post<UserRow>("/users", { email: e.email, display_name: e.display_name, role: e.role, password: e.password }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["users"] });
      setEdit(null);
      toast("Utilisateur enregistré");
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Erreur inattendue"),
  });

  if (isLoading) return <Loading />;

  const q = search.trim().toLowerCase();
  const rows = (data ?? []).filter((u) => !q || u.display_name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q));

  const columns: Column<UserRow>[] = [
    { key: "name", header: "Nom", render: (u) => u.display_name, sortValue: (u) => u.display_name },
    { key: "email", header: "Email", render: (u) => u.email, sortValue: (u) => u.email },
    { key: "role", header: "Rôle", render: (u) => ROLE_LABELS[u.role], sortValue: (u) => u.role },
    { key: "active", header: "Statut", render: (u) => (u.active ? <Badge tone="success">Actif</Badge> : <Badge>Désactivé</Badge>) },
  ];

  const open = (u?: UserRow) => {
    setError("");
    setEdit(u ? { id: u.id, email: u.email, display_name: u.display_name, role: u.role, active: u.active, password: "" } : { email: "", display_name: "", role: "UTILISATEUR", active: true, password: "" });
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (edit) save.mutate(edit);
  };

  return (
    <Card
      title="Utilisateurs"
      actions={
        <div className="toolbar">
          <SearchInput value={search} onChange={setSearch} placeholder="Rechercher un utilisateur" />
          <Button variant="primary" onClick={() => open()}>
            Nouveau
          </Button>
        </div>
      }
    >
      <DataTable columns={columns} rows={rows} rowKey={(u) => u.id} onRowClick={open} />
      <Modal
        open={!!edit}
        title={edit?.id ? "Modifier l'utilisateur" : "Nouvel utilisateur"}
        onClose={() => setEdit(null)}
        footer={
          <>
            <Button onClick={() => setEdit(null)}>Annuler</Button>
            <Button variant="primary" type="submit" form="user-form" loading={save.isPending}>
              Enregistrer
            </Button>
          </>
        }
      >
        {edit ? (
          <form id="user-form" onSubmit={submit} className="form-stack">
            <Alert>{error}</Alert>
            <TextField label="Nom affiché" required value={edit.display_name} onChange={(e) => setEdit({ ...edit, display_name: e.target.value })} />
            <TextField label="Email" type="email" required disabled={!!edit.id} value={edit.email} onChange={(e) => setEdit({ ...edit, email: e.target.value })} />
            <SelectField label="Rôle" value={edit.role} options={ROLE_OPTIONS} onChange={(e) => setEdit({ ...edit, role: e.target.value as Role })} />
            <TextField
              label={edit.id ? "Nouveau mot de passe (laisser vide pour ne pas changer)" : "Mot de passe"}
              type="password"
              autoComplete="new-password"
              required={!edit.id}
              minLength={10}
              hint="10 caractères minimum"
              value={edit.password}
              onChange={(e) => setEdit({ ...edit, password: e.target.value })}
            />
            {edit.id ? (
              <Checkbox label="Compte actif" checked={edit.active} disabled={edit.id === me?.id} onChange={(e) => setEdit({ ...edit, active: e.target.checked })} />
            ) : null}
          </form>
        ) : null}
      </Modal>
    </Card>
  );
}
