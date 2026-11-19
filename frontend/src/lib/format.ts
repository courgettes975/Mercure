// Formatage d'affichage. Les montants arrivent en centimes (entiers).

// Centimes -> "1 234,56 €" (séparateur de milliers, virgule décimale).
export function formatMoney(cents: number): string {
  const neg = cents < 0;
  const abs = Math.abs(cents);
  const euros = Math.floor(abs / 100)
    .toString()
    .replace(/\B(?=(\d{3})+(?!\d))/g, " ");
  const c = (abs % 100).toString().padStart(2, "0");
  return `${neg ? "-" : ""}${euros},${c} €`;
}

// Minutes -> "7h", "7h 30min" ou "45min".
export function formatMinutes(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}min`;
  return m === 0 ? `${h}h` : `${h}h ${m.toString().padStart(2, "0")}min`;
}

/** Taux en points de base (2000) -> "20" ; affichage avec virgule. */
export function bpToPercent(bp: number): string {
  const s = (bp / 100).toFixed(2).replace(/\.?0+$/, "");
  return s.replace(".", ",");
}

/** "20,5" -> 2050 ; null si invalide. */
export function percentToBp(input: string): number | null {
  const s = input.trim().replace(",", ".");
  if (!/^\d+(\.\d{1,2})?$/.test(s)) return null;
  const [i, f = ""] = s.split(".");
  return Number(i) * 100 + Number(f.padEnd(2, "0"));
}

// Horodatage ISO -> date et heure courtes en français.
export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
}

// Taille de fichier lisible (o, Ko, Mo).
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} o`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} Ko`;
  return `${(n / 1024 / 1024).toFixed(1).replace(".", ",")} Mo`;
}

// Première lettre du nom pour l'avatar.
export function initials(name: string): string {
  return name.trim().charAt(0).toUpperCase() || "?";
}

/** "1 234,5" -> 123450 centimes ; null si invalide. */
export function parseMoney(input: string): number | null {
  const s = input.replace(/[\s €]/g, "").replace(",", ".");
  if (s === "") return 0;
  if (!/^-?\d+(\.\d{1,2})?$/.test(s)) return null;
  const neg = s.startsWith("-");
  const [i, f = ""] = s.replace("-", "").split(".");
  const v = Number(i) * 100 + Number(f.padEnd(2, "0"));
  return neg ? -v : v;
}

/** Centimes -> "1234,50" pour un champ de saisie. */
export function moneyInput(cents: number): string {
  if (!cents) return "";
  const s = (Math.abs(cents) / 100).toFixed(2).replace(".", ",");
  return (cents < 0 ? "-" : "") + s.replace(/,00$/, "");
}

/** "7", "7,5", "7h30", "7:30", "45min" -> minutes ; null si invalide. */
export function parseDuration(input: string): number | null {
  const s = input.trim().toLowerCase().replace(/\s/g, "");
  if (s === "") return 0;
  let m = s.match(/^(\d+)(?:h|:)(\d{1,2})?(?:min)?$/);
  if (m) return Number(m[1]) * 60 + Number(m[2] ?? 0);
  m = s.match(/^(\d+)min$/);
  if (m) return Number(m[1]);
  m = s.match(/^(\d+)(?:[.,](\d+))?$/);
  if (m) return Math.round(Number(`${m[1]}.${m[2] ?? 0}`) * 60);
  return null;
}

/** Minutes -> "7h30" pour un champ de saisie. */
export function durationInput(minutes: number): string {
  if (!minutes) return "";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h}h${String(m).padStart(2, "0")}` : `${h}`;
}

/** Quantité en millièmes -> "1,5". */
export function formatQty(milli: number): string {
  return String(milli / 1000).replace(".", ",");
}

/** "1,5" -> 1500 ; null si invalide. */
export function parseQty(input: string): number | null {
  const s = input.trim().replace(",", ".");
  if (!/^\d+(\.\d{1,3})?$/.test(s)) return null;
  return Math.round(Number(s) * 1000);
}

// "2026-11-18" -> "18/11/2026".
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const [y, m, d] = iso.slice(0, 10).split("-");
  return d ? `${d}/${m}/${y}` : iso;
}

// Date du jour "AAAA-MM-JJ" en heure locale.
export function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// Date ISO + n jours (gère les changements de mois).
export function addDaysIso(iso: string, days: number): string {
  const [y, m, d] = iso.split("-").map(Number);
  const dt = new Date(y, m - 1, d + days);
  return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
}

/** Échéance relative comme Odoo : "Dans 9 jours", "Il y a 3 jours", "Aujourd'hui". */
export function relativeDue(iso: string): { text: string; late: boolean } {
  const [y, m, d] = iso.split("-").map(Number);
  const due = new Date(y, m - 1, d).getTime();
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const days = Math.round((due - today) / 86400000);
  if (days === 0) return { text: "Aujourd'hui", late: false };
  if (days > 0) return { text: days === 1 ? "Demain" : `Dans ${days} jours`, late: false };
  const n = -days;
  if (n < 31) return { text: n === 1 ? "Hier" : `Il y a ${n} jours`, late: true };
  const months = Math.round(n / 30);
  return { text: months === 1 ? "Le mois dernier" : `Il y a ${months} mois`, late: true };
}
