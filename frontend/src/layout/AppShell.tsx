import { NavLink, Outlet } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import { ROLE_LABELS, type Role } from "../api/types";
import { initials } from "../lib/format";

// Entrée du menu latéral ; `roles` restreint l'affichage (le serveur contrôle réellement l'accès).
interface NavItem {
  to: string;
  label: string;
  icon: string;
  soon?: string; // lot prévu
  roles?: Role[];
}

// Menu principal dans l'ordre d'affichage.
const NAV: NavItem[] = [
  { to: "/", label: "Tableau de bord", icon: "▦" },
  { to: "/contacts", label: "Contacts", icon: "◉" },
  { to: "/missions", label: "Missions de formation", icon: "◈" },
  { to: "/projets", label: "Projets", icon: "◧" },
  { to: "/temps", label: "Feuilles de temps", icon: "◷" },
  { to: "/facturation", label: "Facturation", icon: "€", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "/depenses", label: "Dépenses", icon: "−", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "/statistiques", label: "Statistiques", icon: "▤", roles: ["ADMIN", "GESTIONNAIRE"] },
  { to: "/parametres", label: "Paramètres", icon: "⚙" },
];

// Coquille commune : barre latérale (menu, utilisateur, déconnexion) + zone de page (Outlet).
export function AppShell() {
  const { user, logout } = useAuth();
  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">E</span>
          <span>Mercure</span>
        </div>
        <nav aria-label="Navigation principale">
          // Les entrées réservées à un rôle sont masquées aux autres.
          {NAV.filter((n) => !n.roles || (user && n.roles.includes(user.role))).map((n) =>
            n.soon ? (
              <span key={n.to} className="nav-item disabled" title={`Disponible au ${n.soon}`}>
                <span className="nav-icon" aria-hidden>
                  {n.icon}
                </span>
                {n.label}
                <span className="nav-soon">{n.soon}</span>
              </span>
            ) : (
              <NavLink key={n.to} to={n.to} end={n.to === "/"} className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}>
                <span className="nav-icon" aria-hidden>
                  {n.icon}
                </span>
                {n.label}
              </NavLink>
            ),
          )}
        </nav>
        <div className="sidebar-user">
          <span className="avatar">{initials(user?.display_name ?? "")}</span>
          <div className="sidebar-user-text">
            <strong>{user?.display_name}</strong>
            <small>{user ? ROLE_LABELS[user.role] : ""}</small>
          </div>
          <button className="icon-btn" onClick={logout} title="Se déconnecter" aria-label="Se déconnecter">
            ⎋
          </button>
        </div>
      </aside>
      <main className="main">
        <Outlet />
      </main>
    </div>
  );
}
