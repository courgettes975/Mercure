import { FormEvent, useState } from "react";
import { api, ApiError } from "../../api/client";
import { ROLE_LABELS } from "../../api/types";
import { useAuth } from "../../auth/AuthContext";
import { Alert, Button, Card, TextField } from "../../components/ui";
import { useToast } from "../../components/Toast";

// Mon compte : changement de son propre mot de passe (ancien mot de passe exigé).
export function AccountPage() {
  const { user } = useAuth();
  const toast = useToast();
  const [form, setForm] = useState({ current: "", next: "", confirm: "" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError("");
    if (form.next !== form.confirm) {
      setError("Les deux mots de passe ne correspondent pas");
      return;
    }
    setLoading(true);
    try {
      await api.post("/auth/password", { current_password: form.current, new_password: form.next });
      setForm({ current: "", next: "", confirm: "" });
      toast("Mot de passe modifié");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Erreur inattendue");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="form-stack narrow">
      <Card title="Mon compte">
        <dl className="details">
          <dt>Nom</dt>
          <dd>{user?.display_name}</dd>
          <dt>Email</dt>
          <dd>{user?.email}</dd>
          <dt>Rôle</dt>
          <dd>{user ? ROLE_LABELS[user.role] : ""}</dd>
        </dl>
      </Card>
      <Card title="Changer de mot de passe">
        <form onSubmit={submit} className="form-stack">
          <Alert>{error}</Alert>
          <TextField label="Mot de passe actuel" type="password" autoComplete="current-password" required value={form.current} onChange={(e) => setForm({ ...form, current: e.target.value })} />
          <TextField label="Nouveau mot de passe" type="password" autoComplete="new-password" required minLength={10} hint="10 caractères minimum" value={form.next} onChange={(e) => setForm({ ...form, next: e.target.value })} />
          <TextField label="Confirmation" type="password" autoComplete="new-password" required value={form.confirm} onChange={(e) => setForm({ ...form, confirm: e.target.value })} />
          <div className="form-actions">
            <Button variant="primary" type="submit" loading={loading}>
              Modifier le mot de passe
            </Button>
          </div>
        </form>
      </Card>
    </div>
  );
}
