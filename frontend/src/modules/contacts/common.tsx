import type { ContactKind } from "../../api/types";
import { Badge } from "../../components/ui";
import { initials } from "../../lib/format";

// Palette des avatars (couleur stable choisie d'après le nom).
const COLORS = ["#5b4b9a", "#2f6f9f", "#1e8a4c", "#b26b00", "#a23b5f", "#3d6b6b", "#7a5c2e", "#3b4f9a"];

// Couleur déterministe pour un nom (même nom -> même couleur).
export function colorFor(name: string) {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return COLORS[h % COLORS.length];
}

// Rond coloré avec l'initiale ; carré pour une société.
export function Avatar({ name, kind, size = 26 }: { name: string; kind?: ContactKind; size?: number }) {
  return (
    <span
      className={`avatar ${kind && kind !== "PARTICULIER" ? "avatar-square" : ""}`}
      style={{ background: colorFor(name), width: size, height: size, fontSize: size * 0.45 }}
      aria-hidden
    >
      {initials(name)}
    </span>
  );
}

// Pastilles Client / Prospect / Stagiaire.
export function RoleBadges({ c }: { c: { is_client: boolean; is_prospect: boolean; is_stagiaire: boolean } }) {
  return (
    <span className="chips">
      {c.is_client ? <Badge tone="success">Client</Badge> : null}
      {c.is_prospect ? <Badge tone="warning">Prospect</Badge> : null}
      {c.is_stagiaire ? <Badge tone="info">Stagiaire</Badge> : null}
    </span>
  );
}

const FIELD_LABELS: Record<string, string> = {
  kind: "Type",
  parent_id: "Société",
  name: "Nom",
  first_name: "Prénom",
  is_client: "Client",
  is_prospect: "Prospect",
  is_stagiaire: "Stagiaire",
  job_title: "Fonction",
  email: "Email",
  phone: "Téléphone",
  mobile: "Mobile",
  street: "Adresse",
  street2: "Complément",
  zip: "Code postal",
  city: "Ville",
  country: "Pays",
  website: "Site web",
  notes: "Notes",
  customer_number: "N° client",
  siren: "SIREN",
  siret: "SIRET",
  vat_number: "TVA",
  duns: "DUNS",
  tags: "Étiquettes",
  archived_at: "Archivé le",
};

// Rend une valeur JSON lisible dans l'historique.
function show(v: unknown): string {
  if (v === null || v === undefined || v === "") return "vide";
  if (v === true) return "oui";
  if (v === false) return "non";
  if (Array.isArray(v)) return v.length ? v.join(", ") : "aucune";
  return String(v);
}

/** Transforme le JSON du journal en lignes lisibles. */
// Transforme le JSON "changes" du journal en phrases "champ : avant -> après".
export function describeChanges(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const obj = JSON.parse(raw) as Record<string, unknown>;
    return Object.entries(obj).map(([k, v]) => {
      const label = FIELD_LABELS[k] ?? k;
      if (v && typeof v === "object" && !Array.isArray(v) && "old" in v && "new" in v) {
        const c = v as { old: unknown; new: unknown };
        return `${label} : ${show(c.old)} -> ${show(c.new)}`;
      }
      return `${label} : ${show(v)}`;
    });
  } catch {
    return [raw];
  }
}
