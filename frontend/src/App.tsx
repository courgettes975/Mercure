// Routage de l'application : une route par écran, protégée par rôle quand l'écran est réservé.
import { Navigate, Route, Routes } from "react-router-dom";
import { hasRole, useAuth } from "./auth/AuthContext";
import { Loading } from "./components/ui";
import { AppShell } from "./layout/AppShell";
import { LoginPage, SetupPage } from "./modules/auth/AuthPages";
import { ContactPage } from "./modules/contacts/ContactPage";
import { ContactsListPage } from "./modules/contacts/ContactsListPage";
import { ImportPage } from "./modules/contacts/ImportPage";
import { BillFromProjectPage, InvoicePage, InvoicesListPage, ToInvoicePage } from "./modules/invoices/InvoicesPages";
import { MonthlyBillingPage } from "./modules/invoices/MonthlyBillingPage";
import { InvoiceOverviewPage } from "./modules/invoices/OverviewPage";
import { MissionPage } from "./modules/missions/MissionPage";
import { MissionsListPage } from "./modules/missions/MissionsListPage";
import { ProjectPage, ProjectsListPage } from "./modules/projects/ProjectsPages";
import { TimesheetsPage } from "./modules/timesheets/TimesheetsPage";
import { DashboardPage } from "./modules/dashboard/DashboardPage";
import { StatsPage } from "./modules/stats/StatsPage";
import { ExpensesPage } from "./modules/expenses/ExpensesPage";
import { AccountPage } from "./modules/settings/AccountPage";
import { AuditPage } from "./modules/settings/AuditPage";
import { BackupsPage } from "./modules/settings/BackupsPage";
import { StoragePage } from "./modules/settings/StoragePage";
import { DemoPage } from "./modules/settings/DemoPage";
import { CompanyPage } from "./modules/settings/CompanyPage";
import { SettingsLayout } from "./modules/settings/SettingsLayout";
import { SoftwarePage } from "./modules/settings/SoftwarePage";
import { ProductsPage } from "./modules/settings/ProductsPage";
import { TrainingCentersPage } from "./modules/settings/TrainingCentersPage";
import { UsersPage } from "./modules/settings/UsersPage";
import type { Role } from "./api/types";

// Garde d'affichage : redirige vers l'accueil si le rôle ne convient pas (le serveur reste la vraie barrière).
function RequireRole({ roles, children }: { roles: Role[]; children: React.ReactElement }) {
  const { user } = useAuth();
  return hasRole(user, ...roles) ? children : <Navigate to="/" replace />;
}

// Trois états possibles avant l'application : chargement, première installation, connexion.
export function App() {
  const { ready, user, setupRequired } = useAuth();
  if (!ready) return <Loading />;
  if (setupRequired) return <SetupPage />;
  if (!user) return <LoginPage />;

  return (
    <Routes>
      // Toutes les pages partagent la coquille (menu latéral, en-tête).
      <Route element={<AppShell />}>
        <Route index element={<DashboardPage />} />
        <Route path="contacts" element={<ContactsListPage />} />
        <Route path="contacts/import" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><ImportPage /></RequireRole>} />
        <Route path="contacts/:id" element={<ContactPage />} />
        <Route path="projets" element={<ProjectsListPage />} />
        <Route path="projets/:id" element={<ProjectPage />} />
        <Route path="missions" element={<MissionsListPage />} />
        <Route path="missions/:id" element={<MissionPage />} />
        <Route path="temps" element={<TimesheetsPage />} />
        <Route path="statistiques" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><StatsPage /></RequireRole>} />
        <Route path="depenses" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><ExpensesPage /></RequireRole>} />
        <Route path="facturation" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><InvoicesListPage /></RequireRole>} />
        <Route path="facturation/a-facturer" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><ToInvoicePage /></RequireRole>} />
        <Route path="facturation/mensuelle" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><MonthlyBillingPage /></RequireRole>} />
        <Route path="facturation/etat" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><InvoiceOverviewPage /></RequireRole>} />
        <Route path="facturation/depuis-projet/:projectId" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><BillFromProjectPage /></RequireRole>} />
        <Route path="facturation/:id" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><InvoicePage /></RequireRole>} />
        // Paramètres : sous-onglets avec leurs propres droits.
        <Route path="parametres" element={<SettingsLayout />}>
          <Route path="entreprise" element={<CompanyPage />} />
          <Route path="utilisateurs" element={<RequireRole roles={["ADMIN"]}><UsersPage /></RequireRole>} />
          <Route path="logiciels" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><SoftwarePage /></RequireRole>} />
          <Route path="produits" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><ProductsPage /></RequireRole>} />
          <Route path="centres" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><TrainingCentersPage /></RequireRole>} />
          <Route path="sauvegardes" element={<RequireRole roles={["ADMIN", "GESTIONNAIRE"]}><BackupsPage /></RequireRole>} />
          <Route path="journal" element={<RequireRole roles={["ADMIN"]}><AuditPage /></RequireRole>} />
          <Route path="stockage" element={<RequireRole roles={["ADMIN"]}><StoragePage /></RequireRole>} />
          <Route path="demo" element={<RequireRole roles={["ADMIN"]}><DemoPage /></RequireRole>} />
          <Route path="compte" element={<AccountPage />} />
        </Route>
        // Route inconnue : retour à l'accueil.
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
