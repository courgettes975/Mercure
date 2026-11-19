// Types partagés avec l'API (miroir des structures Rust sérialisées en JSON).
// Montants en centimes, durées en minutes, quantités en millièmes, taux en points de base, dates "AAAA-MM-JJ".
export type Role = "ADMIN" | "GESTIONNAIRE" | "UTILISATEUR";

// Libellés affichés des rôles.
export const ROLE_LABELS: Record<Role, string> = {
  ADMIN: "Administrateur",
  GESTIONNAIRE: "Gestionnaire",
  UTILISATEUR: "Utilisateur",
};

// Utilisateur connecté (/auth/me).
export interface CurrentUser {
  id: number;
  email: string;
  display_name: string;
  role: Role;
}

// Compte listé dans Paramètres > Utilisateurs.
export interface UserRow {
  id: number;
  email: string;
  display_name: string;
  role: Role;
  active: boolean;
  created_at: string;
}

// Paramètres de l'entreprise (Paramètres > Entreprise).
export interface CompanySettings {
  legal_name: string;
  address_line1: string;
  address_line2: string;
  zip: string;
  city: string;
  country: string;
  siren: string;
  siret: string;
  vat_number: string;
  nda: string;
  email: string;
  phone: string;
  website: string;
  bank_name: string;
  iban: string;
  bic: string;
  payment_terms: string;
  default_due_days: number;
  vat_regime: "FRANCHISE" | "ASSUJETTI";
  default_vat_rate_bp: number;
  vat_exemption_mention: string;
  legal_mentions: string;
  invoice_prefix: string;
  invoice_number_width: number;
  logo_file: string | null;
  einvoicing_enabled: boolean;
  einvoicing_platform: string;
  vat_on_debits: boolean;
  default_operation_category: "SERVICES" | "BIENS" | "MIXTE";
}

// Centre de formation de la liste de référence.
export interface TrainingCenter {
  id: number;
  name: string;
  kind: string;
  city: string;
  missions_count: number;
}

// Produit / prestation de la liste de référence (pré-remplit les lignes de facture).
export interface Product {
  id: number;
  name: string;
  description: string;
  unit: string;
  unit_price: number;
  vat_rate_bp: number;
  active: boolean;
  position: number;
}

// Logiciel enseigné.
export interface Software {
  id: number;
  name: string;
  active: boolean;
}

// Fichier de sauvegarde.
export interface BackupFile {
  name: string;
  size: number;
  kind: "auto" | "manuel";
}

// Ligne du journal d'audit.
export interface AuditEntry {
  id: number;
  user_id: number | null;
  user_name: string | null;
  action: string;
  entity_type: string;
  entity_id: string | null;
  changes: string | null;
  created_at: string;
}

// Réponse paginée générique.
export interface Page<T> {
  items: T[];
  total: number;
  page: number;
  per_page: number;
}

// Types de contact et leurs libellés.
export type ContactKind = "PARTICULIER" | "ENTREPRISE" | "ORGANISME";

export const KIND_LABELS: Record<ContactKind, string> = {
  PARTICULIER: "Particulier",
  ENTREPRISE: "Entreprise",
  ORGANISME: "Organisme",
};

// Étiquette.
export interface Tag {
  id: number;
  name: string;
}

// Saisie d'un contact (création / modification).
export interface ContactInput {
  kind: ContactKind;
  parent_id: number | null;
  name: string;
  first_name: string;
  is_client: boolean;
  is_prospect: boolean;
  is_stagiaire: boolean;
  job_title: string;
  email: string;
  phone: string;
  mobile: string;
  street: string;
  street2: string;
  zip: string;
  city: string;
  country: string;
  website: string;
  notes: string;
  siren: string;
  siret: string;
  vat_number: string;
  duns: string;
  tag_ids: number[];
}

// Fiche contact complète renvoyée par le serveur.
export interface ContactDetail extends Omit<ContactInput, "tag_ids"> {
  id: number;
  parent_name: string | null;
  customer_number: string | null;
  archived_at: string | null;
  is_demo: boolean;
  created_at: string;
  updated_at: string;
  display_name: string;
  tags: Tag[];
  children_count: number;
}

// Ligne de la liste des contacts.
export interface ContactListItem {
  id: number;
  kind: ContactKind;
  name: string;
  first_name: string;
  parent_id: number | null;
  parent_name: string | null;
  email: string;
  phone: string;
  mobile: string;
  city: string;
  country: string;
  is_client: boolean;
  is_prospect: boolean;
  is_stagiaire: boolean;
  customer_number: string | null;
  archived_at: string | null;
  display_name: string;
  tags: Tag[];
}

// Note libre d'un contact.
export interface NoteItem {
  id: number;
  author_name: string | null;
  body: string;
  created_at: string;
}

// Entrée d'historique d'un contact.
export interface HistoryItem {
  id: number;
  user_name: string | null;
  action: string;
  changes: string | null;
  created_at: string;
}

// ---------- Projets ----------
// Statuts de projet et leur rendu (libellé + couleur).
export type ProjectStatus = "BROUILLON" | "EN_COURS" | "TERMINE" | "ANNULE";
export const PROJECT_STATUS: Record<ProjectStatus, { label: string; tone: "neutral" | "info" | "success" | "danger" }> = {
  BROUILLON: { label: "Brouillon", tone: "neutral" },
  EN_COURS: { label: "En cours", tone: "info" },
  TERMINE: { label: "Terminé", tone: "success" },
  ANNULE: { label: "Annulé", tone: "danger" },
};
// Utilisateur affectable (intervenant, formateur).
export interface Person {
  id: number;
  display_name: string;
}
// Tâche d'un projet.
export interface Task {
  id: number;
  project_id: number;
  name: string;
  active: boolean;
}
// Projet (liste et fiche).
export interface Project {
  id: number;
  reference: string;
  kind: "PROJET" | "FORMATION";
  name: string;
  client_id: number;
  client_name: string;
  description: string;
  start_date: string | null;
  end_date: string | null;
  status: ProjectStatus;
  billable: boolean;
  hourly_rate: number;
  archived_at: string | null;
  mission_id: number | null;
  minutes_total: number;
  minutes_billable: number;
  amount_uninvoiced: number;
  created_at: string;
  members?: Person[];
  tasks?: Task[];
}

// ---------- Missions ----------
// Statuts de mission, leur rendu et leur ordre d'affichage (kanban).
export type MissionStatus =
  | "BROUILLON" | "A_PLANIFIER" | "PLANIFIEE" | "CONFIRMEE" | "EN_COURS" | "REALISEE" | "A_FACTURER" | "FACTUREE" | "ANNULEE";
export const MISSION_STATUS: Record<MissionStatus, { label: string; tone: "neutral" | "info" | "success" | "warning" | "danger" }> = {
  BROUILLON: { label: "Brouillon", tone: "neutral" },
  A_PLANIFIER: { label: "À planifier", tone: "warning" },
  PLANIFIEE: { label: "Planifiée", tone: "info" },
  CONFIRMEE: { label: "Confirmée", tone: "info" },
  EN_COURS: { label: "En cours", tone: "info" },
  REALISEE: { label: "Réalisée", tone: "success" },
  A_FACTURER: { label: "À facturer", tone: "warning" },
  FACTUREE: { label: "Facturée", tone: "success" },
  ANNULEE: { label: "Annulée", tone: "danger" },
};
export const MISSION_STATUS_ORDER: MissionStatus[] = [
  "BROUILLON", "A_PLANIFIER", "PLANIFIEE", "CONFIRMEE", "EN_COURS", "REALISEE", "A_FACTURER", "FACTUREE", "ANNULEE",
];
// Libellés des listes fixes des missions (modalité, format, tarification, frais, documents).
export const MODALITY_LABELS = { PRESENTIEL: "Présentiel", DISTANCIEL: "Distanciel", HYBRIDE: "Hybride" } as const;
export const FORMAT_LABELS = { INDIVIDUEL: "Individuel", GROUPE: "En groupe" } as const;
export const PRICING_LABELS = { HORAIRE: "Tarif horaire", JOURNALIER: "Tarif journalier", FORFAIT: "Forfait" } as const;
export const EXPENSE_LABELS = {
  DEPLACEMENT: "Déplacement",
  KILOMETRIQUE: "Frais kilométriques",
  HEBERGEMENT: "Hébergement",
  RESTAURATION: "Restauration",
  AUTRE: "Autre",
} as const;
export const DOC_LABELS = {
  CONVENTION: "Convention de formation",
  CONVOCATION: "Convocation",
  PROGRAMME: "Programme",
  DEVIS: "Devis",
  BON_COMMANDE: "Bon de commande",
  EMARGEMENT: "Feuille d'émargement",
  ATTESTATION: "Attestation de présence",
  CERTIFICAT: "Certificat de réalisation",
  EVALUATION: "Évaluation",
  FACTURE: "Facture",
  CLIENT: "Document client",
  AUTRE: "Autre",
} as const;

// Saisie d'une mission (tous les champs modifiables).
export interface MissionInput {
  title: string;
  client_id: number;
  contact_id: number | null;
  center_id: number | null;
  trainer_id: number | null;
  software_id: number | null;
  description: string;
  start_date: string | null;
  end_date: string | null;
  os: "" | "PC" | "MAC";
  modality: keyof typeof MODALITY_LABELS;
  group_format: keyof typeof FORMAT_LABELS;
  provide_pc: boolean;
  recording: boolean;
  planned_minutes: number;
  days_count: number;
  schedule_text: string;
  address: string;
  room: string;
  visio_link: string;
  pricing_mode: keyof typeof PRICING_LABELS;
  hourly_rate: number;
  daily_rate: number;
  flat_fee: number;
  billable: boolean;
  special_conditions: string;
  internal_notes: string;
  annotations: string;
  file_created: boolean;
  first_call_done: boolean;
  invoice_sent: boolean;
  objectives: string;
  prerequisites: string;
  program: string;
  methods: string;
  means: string;
  evaluation_terms: string;
  evaluation_result: string;
  satisfaction: string;
  trainer_comment: string;
}

// Séance de formation.
export interface Session {
  id: number;
  mission_id: number;
  date: string;
  start_time: string;
  end_time: string;
  break_minutes: number;
  duration_minutes: number;
  trainer_id: number | null;
  trainer_name: string | null;
  location: string;
  status: "PREVUE" | "REALISEE" | "ANNULEE";
  notes: string;
  timesheet_id: number | null;
  invoiced: boolean;
}

// Stagiaire inscrit avec son suivi.
export interface Participant {
  id: number;
  partner_id: number;
  name: string;
  first_name: string;
  company: string | null;
  email: string;
  phone: string;
  job_title: string;
  progress_status: "EN_COURS" | "TERMINE" | "ABANDON";
  evaluation: string;
  comment: string;
  minutes_done: number;
  sessions_present: number;
  sessions_absent: number;
}

// Frais d'une mission.
export interface Expense {
  id: number;
  kind: keyof typeof EXPENSE_LABELS;
  label: string;
  expense_date: string | null;
  quantity_milli: number;
  unit_price: number;
  amount: number;
  billable: boolean;
  invoiced: boolean;
}

// Pièce jointe.
export interface DocumentItem {
  id: number;
  category: keyof typeof DOC_LABELS;
  file_name: string;
  mime_type: string;
  size: number;
  uploaded_by_name: string | null;
  created_at: string;
}

// Facture liée affichée dans la fiche mission.
export interface InvoiceRef {
  id: number;
  number: string | null;
  kind: "FACTURE" | "AVOIR";
  status: InvoiceStatus;
  invoice_date: string;
  total_ttc: number;
}

// Fiche mission complète (saisie + données calculées + sous-listes).
export interface MissionDetail extends MissionInput {
  id: number;
  project_id: number;
  reference: string;
  status: MissionStatus;
  client_name: string;
  contact_name: string | null;
  center_name: string | null;
  trainer_name: string | null;
  software_name: string | null;
  archived_at: string | null;
  sessions: Session[];
  participants: Participant[];
  attendance: { session_id: number; participant_id: number; present: boolean }[];
  expenses: Expense[];
  documents: DocumentItem[];
  invoices: InvoiceRef[];
  totals: {
    planned_fee: number;
    expenses_total: number;
    planned_total: number;
    sessions_minutes: number;
    realized_minutes: number;
    realized_amount: number;
    invoiced_ht: number;
    uninvoiced_amount: number;
  };
}

// Ligne de la liste des missions.
export interface MissionListItem {
  id: number;
  project_id: number;
  reference: string;
  status: MissionStatus;
  title: string;
  client_id: number;
  client_name: string;
  center_id: number | null;
  center_name: string | null;
  invoice_sent: boolean;
  trainer_name: string | null;
  software_name: string | null;
  modality: keyof typeof MODALITY_LABELS;
  group_format: keyof typeof FORMAT_LABELS;
  start_date: string | null;
  end_date: string | null;
  first_session: string | null;
  last_session: string | null;
  planned_minutes: number;
  realized_minutes: number;
  participants: number;
  expenses_total: number;
  uninvoiced_amount: number;
}

// Séance pour la vue calendrier.
export interface CalendarSession {
  id: number;
  mission_id: number;
  reference: string;
  title: string;
  client_name: string;
  date: string;
  start_time: string;
  end_time: string;
  duration_minutes: number;
  status: Session["status"];
  trainer_name: string | null;
}

// ---------- Temps ----------
// Ligne de feuille de temps enrichie.
export interface TimesheetRow {
  id: number;
  date: string;
  user_id: number;
  user_name: string;
  project_id: number;
  project_name: string;
  project_reference: string;
  client_id: number;
  client_name: string;
  task_id: number | null;
  task_name: string | null;
  session_id: number | null;
  mission_id: number | null;
  center_id: number | null;
  center_name: string | null;
  description: string;
  duration_minutes: number;
  billable: boolean;
  hourly_rate: number;
  amount: number;
  status: "SAISI" | "VALIDE";
  invoice_line_id: number | null;
  invoice_id: number | null;
  invoice_number: string | null;
  invoiced_at: string | null;
}
// Projet / tâche proposé à la saisie.
export interface TimesheetChoice {
  project_id: number;
  project_name: string;
  client_name: string;
  kind: "PROJET" | "FORMATION";
  task_id: number | null;
  task_name: string | null;
}

// ---------- Facturation ----------
// Statuts de transmission d'une facture électronique (plateforme agréée) et leur rendu.
export type EinvoiceStatus = "A_TRANSMETTRE" | "TRANSMISE" | "ACCEPTEE" | "REFUSEE" | "HORS_CHAMP";
export const EINVOICE_STATUS: Record<EinvoiceStatus, { label: string; tone: "info" | "success" | "warning" | "danger" | "neutral" }> = {
  A_TRANSMETTRE: { label: "À transmettre", tone: "warning" },
  TRANSMISE: { label: "Transmise", tone: "info" },
  ACCEPTEE: { label: "Acceptée", tone: "success" },
  REFUSEE: { label: "Refusée", tone: "danger" },
  HORS_CHAMP: { label: "Hors champ (particulier)", tone: "neutral" },
};
export const OPERATION_CATEGORY_LABELS: Record<"SERVICES" | "BIENS" | "MIXTE", string> = { SERVICES: "Prestation de services", BIENS: "Livraison de biens", MIXTE: "Mixte (biens et services)" };

// Statuts de facture et leur rendu.
export type InvoiceStatus = "BROUILLON" | "VALIDEE" | "PAYEE" | "ANNULEE";
export const INVOICE_STATUS: Record<InvoiceStatus, { label: string; tone: "neutral" | "info" | "success" | "danger" }> = {
  BROUILLON: { label: "Brouillon", tone: "neutral" },
  VALIDEE: { label: "Validée", tone: "info" },
  PAYEE: { label: "Payée", tone: "success" },
  ANNULEE: { label: "Annulée", tone: "danger" },
};
// Ligne de facture.
export interface InvoiceLine {
  id?: number;
  position?: number;
  description: string;
  quantity_milli: number;
  unit: string;
  unit_price: number;
  vat_rate_bp: number;
  total_ht?: number;
  total_vat?: number;
  total_ttc?: number;
  source_type?: "MANUEL" | "TEMPS" | "FRAIS" | "FORFAIT";
  linked_items?: number;
  product_id: number | null;
}
// Paiement reçu.
export interface Payment {
  id: number;
  payment_date: string;
  amount: number;
  method: string;
  reference: string;
}
// Facture ou avoir (en-tête + lignes + paiements).
export interface Invoice {
  id: number;
  number: string | null;
  kind: "FACTURE" | "AVOIR";
  reversed_invoice_id: number | null;
  reversed_number: string | null;
  partner_id: number;
  partner_name: string;
  project_id: number | null;
  project_name: string | null;
  mission_id: number | null;
  billing_name: string;
  billing_address: string;
  invoice_date: string;
  due_date: string;
  status: InvoiceStatus;
  total_ht: number;
  total_vat: number;
  total_ttc: number;
  amount_paid: number;
  amount_due: number;
  payment_terms: string;
  vat_mention: string;
  notes: string;
  validated_at: string | null;
  last_reminder_at: string | null;
  sent_at: string | null;
  billing_siren: string;
  operation_category: "SERVICES" | "BIENS" | "MIXTE";
  einvoice_status: EinvoiceStatus | null;
  einvoice_ref: string;
  einvoice_message: string;
  einvoice_updated_at: string | null;
  credit_note_id: number | null;
  lines?: InvoiceLine[];
  payments?: Payment[];
}
// Éléments à facturer d'un projet.
export interface Billable {
  project_id: number;
  project_name: string;
  client_id: number;
  client_name: string;
  mission_id: number | null;
  pricing_mode: string;
  flat_fee_pending: number;
  timesheets: { id: number; date: string; user_name: string; task_name: string | null; description: string; duration_minutes: number; hourly_rate: number; amount: number }[];
  expenses: { id: number; kind: keyof typeof EXPENSE_LABELS; label: string; quantity_milli: number; unit_price: number; amount: number }[];
  total: number;
}
