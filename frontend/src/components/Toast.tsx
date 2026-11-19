import { createContext, ReactNode, useCallback, useContext, useState } from "react";

// Notifications éphémères en bas d'écran (succès, erreur, information).
type Tone = "success" | "danger" | "info";
interface ToastItem {
  id: number;
  tone: Tone;
  text: string;
}

const Ctx = createContext<(text: string, tone?: Tone) => void>(() => {});

// Fournit la fonction `toast(texte, ton)` à toute l'application et affiche la pile.
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  // Ajoute une notification qui disparaît seule (plus longtemps pour une erreur).
  const push = useCallback((text: string, tone: Tone = "success") => {
    const id = Date.now() + Math.random();
    setItems((l) => [...l, { id, tone, text }]);
    setTimeout(() => setItems((l) => l.filter((t) => t.id !== id)), tone === "danger" ? 7000 : 3500);
  }, []);
  return (
    <Ctx.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        {items.map((t) => (
          <div key={t.id} className={`toast toast-${t.tone}`}>
            {t.text}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}

// Accès à la fonction d'affichage depuis n'importe quel composant.
export const useToast = () => useContext(Ctx);
