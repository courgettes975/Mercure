// Client HTTP : toutes les règles métier restent côté serveur.

// Erreur HTTP avec le code et le message métier renvoyé par le serveur.
export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

// Rappel déclenché sur un 401 : l'application repasse à l'écran de connexion.
let onUnauthorized: (() => void) | null = null;
export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorized = fn;
}

// Appel générique : JSON ou FormData, cookie de session inclus, erreurs traduites en ApiError.
async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: "same-origin", headers: {} };
  // FormData (envoi de fichier) : le navigateur fixe lui-même le Content-Type.
  if (body instanceof FormData) {
    init.body = body;
  } else if (body !== undefined) {
    init.body = JSON.stringify(body);
    (init.headers as Record<string, string>)["Content-Type"] = "application/json";
  }
  let res: Response;
  try {
    res = await fetch(`/api${url}`, init);
  } catch {
    throw new ApiError(0, "Serveur injoignable. Mercure est-il lancé ?");
  }
  const text = await res.text();
  const data = text ? safeJson(text) : null;
  if (!res.ok) {
    // Session expirée (hors routes d'authentification) : déconnexion côté interface.
    if (res.status === 401 && onUnauthorized && !url.startsWith("/auth/")) onUnauthorized();
    const msg = data && typeof data === "object" && "error" in data ? String((data as { error: unknown }).error) : `Erreur ${res.status}`;
    throw new ApiError(res.status, msg);
  }
  return data as T;
}

// Analyse JSON tolérante (corps vide ou non JSON).
function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

// Façade utilisée partout : api.get / post / put / patch / del.
export const api = {
  get: <T>(url: string) => request<T>("GET", url),
  post: <T>(url: string, body?: unknown) => request<T>("POST", url, body),
  put: <T>(url: string, body?: unknown) => request<T>("PUT", url, body),
  patch: <T>(url: string, body?: unknown) => request<T>("PATCH", url, body),
  del: <T>(url: string) => request<T>("DELETE", url),
};
