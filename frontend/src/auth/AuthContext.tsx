import { createContext, ReactNode, useCallback, useContext, useEffect, useState } from "react";
import { api, setUnauthorizedHandler } from "../api/client";
import type { CurrentUser, Role } from "../api/types";

// État d'authentification partagé par toute l'interface.
interface AuthState {
  user: CurrentUser | null;
  setupRequired: boolean;
  ready: boolean;
  setUser: (u: CurrentUser | null) => void;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
}

const Ctx = createContext<AuthState | null>(null);

// Charge l'état au démarrage (/auth/status puis /auth/me) et l'expose via le contexte.
export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<CurrentUser | null>(null);
  const [setupRequired, setSetupRequired] = useState(false);
  const [ready, setReady] = useState(false);

  // Relit l'état serveur ; `ready` passe à vrai même en cas d'erreur pour afficher l'écran de connexion.
  const refresh = useCallback(async () => {
    try {
      const st = await api.get<{ setup_required: boolean }>("/auth/status");
      setSetupRequired(st.setup_required);
      if (!st.setup_required) {
        try {
          setUser(await api.get<CurrentUser>("/auth/me"));
        } catch {
          setUser(null);
        }
      }
    } finally {
      setReady(true);
    }
  }, []);

  // Au montage : brancher la déconnexion automatique sur 401, puis charger l'état.
  useEffect(() => {
    setUnauthorizedHandler(() => setUser(null));
    refresh();
  }, [refresh]);

  // Déconnexion serveur puis locale (même si le serveur est injoignable).
  const logout = async () => {
    await api.post("/auth/logout").catch(() => {});
    setUser(null);
  };

  const set = (u: CurrentUser | null) => {
    setUser(u);
    if (u) setSetupRequired(false);
  };

  return <Ctx.Provider value={{ user, setupRequired, ready, setUser: set, logout, refresh }}>{children}</Ctx.Provider>;
}

// Accès au contexte ; erreur explicite si le fournisseur manque.
export function useAuth() {
  const c = useContext(Ctx);
  if (!c) throw new Error("AuthProvider manquant");
  return c;
}

/** Affichage seulement : la sécurité réelle est vérifiée par le serveur. */
export function hasRole(user: CurrentUser | null, ...roles: Role[]) {
  return !!user && roles.includes(user.role);
}
