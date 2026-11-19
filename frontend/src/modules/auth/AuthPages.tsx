import { FormEvent, useState } from "react";
import { api, ApiError } from "../../api/client";
import type { CurrentUser } from "../../api/types";
import { useAuth } from "../../auth/AuthContext";
import { Alert, Button, TextField } from "../../components/ui";

// Cadre centré commun aux écrans de connexion et de première installation.
function AuthCard({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) {
  return (
    <div className="auth-page">
      <div className="auth-card">
        <div className="brand brand-lg">
          <span className="brand-mark">E</span>
          <span>Mercure</span>
        </div>
        <h1>{title}</h1>
        <p className="muted">{subtitle}</p>
        {children}
      </div>
    </div>
  );
}

// Écran de connexion : envoie email + mot de passe, la session est posée en cookie par le serveur.
export function LoginPage() {
  const { setUser } = useAuth();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  // Soumission : en cas de succès l'utilisateur est mémorisé et l'application s'affiche.
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      setUser(await api.post<CurrentUser>("/auth/login", { email, password }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Erreur inattendue");
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthCard title="Connexion" subtitle="Identifiez-vous pour accéder à Mercure.">
      <form onSubmit={submit} className="form-stack">
        <Alert>{error}</Alert>
        <TextField label="Email" type="email" autoComplete="username" required value={email} onChange={(e) => setEmail(e.target.value)} autoFocus />
        <TextField label="Mot de passe" type="password" autoComplete="current-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
        <Button variant="primary" type="submit" loading={loading}>
          Se connecter
        </Button>
      </form>
    </AuthCard>
  );
}

// Première installation : création du compte administrateur (uniquement si la base est vide).
export function SetupPage() {
  const { setUser } = useAuth();
  const [form, setForm] = useState({ display_name: "", email: "", password: "", confirm: "" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const set = (k: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) => setForm({ ...form, [k]: e.target.value });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError("");
    if (form.password !== form.confirm) {
      setError("Les deux mots de passe ne correspondent pas");
      return;
    }
    setLoading(true);
    try {
      setUser(await api.post<CurrentUser>("/auth/setup", { display_name: form.display_name, email: form.email, password: form.password }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Erreur inattendue");
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthCard title="Première utilisation" subtitle="Créez le compte administrateur. Il pourra ensuite créer les autres utilisateurs.">
      <form onSubmit={submit} className="form-stack">
        <Alert>{error}</Alert>
        <TextField label="Nom affiché" required value={form.display_name} onChange={set("display_name")} autoFocus />
        <TextField label="Email" type="email" autoComplete="username" required value={form.email} onChange={set("email")} />
        <TextField label="Mot de passe" type="password" autoComplete="new-password" required minLength={10} hint="10 caractères minimum" value={form.password} onChange={set("password")} />
        <TextField label="Confirmation du mot de passe" type="password" autoComplete="new-password" required value={form.confirm} onChange={set("confirm")} />
        <Button variant="primary" type="submit" loading={loading}>
          Créer le compte administrateur
        </Button>
      </form>
    </AuthCard>
  );
}
