import { Navigate, NavLink, Outlet, useLocation } from "react-router-dom";
import type { Role } from "../../api/types";
import { hasRole, useAuth } from "../../auth/AuthContext";
import { PageHeader } from "../../components/ui";

export const SETTINGS_TABS: { to: string; label: string; roles: Role[] }[] = [
  { to: "entreprise", label: "Entreprise", roles: ["ADMIN", "GESTIONNAIRE", "UTILISATEUR"] },
  { to: "utilisateurs", label: "Utilisateurs", roles: ["ADMIN"] },
  { to: "logiciels", label: "Logiciels", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "produits", label: "Produits", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "centres", label: "Centres de formation", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "sauvegardes", label: "Sauvegardes", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "stockage", label: "Stockage et réseau", roles: ["ADMIN"] },
  { to: "journal", label: "Journal", roles: ["ADMIN"] },
  { to: "demo", label: "Démo", roles: ["ADMIN"] },
  { to: "compte", label: "Mon compte", roles: ["ADMIN", "GESTIONNAIRE", "UTILISATEUR"] },
];

// Page Paramètres : onglets filtrés par rôle, contenu de l'onglet via Outlet.
export function SettingsLayout() {
  const { user } = useAuth();
  const loc = useLocation();
  const tabs = SETTINGS_TABS.filter((t) => hasRole(user, ...t.roles));
  if (loc.pathname.replace(/\/$/, "") === "/parametres") return <Navigate to={tabs[0].to} replace />;
  return (
    <>
      <PageHeader title="Paramètres" />
      <nav className="tabs" aria-label="Sections des paramètres">
        {tabs.map((t) => (
          <NavLink key={t.to} to={t.to} className={({ isActive }) => `tab ${isActive ? "active" : ""}`}>
            {t.label}
          </NavLink>
        ))}
      </nav>
      <div className="page-body">
        <Outlet />
      </div>
    </>
  );
}
