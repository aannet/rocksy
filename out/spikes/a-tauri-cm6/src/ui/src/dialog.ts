// Modal dialog of the application (never `window.confirm`, which blocks the page and the test driver).
export interface DialogButton {
  id: string;
  label: string;
  kind?: "primary" | "danger";
}

export interface DialogOptions {
  title: string;
  /** Paragraphs (plain text). */
  lines: string[];
  /** Bullet list shown after the paragraphs (plain text). */
  items?: string[];
  buttons: DialogButton[];
  /** Button id returned by Escape. */
  cancelId: string;
}

let open = 0;

/** True while a dialog is shown: global shortcuts are suspended. */
export function dialogOpen(): boolean {
  return open > 0;
}

/** Shows the dialog and resolves with the id of the chosen button. All listeners live on the dialog itself and go with it. */
export function askChoice(opts: DialogOptions): Promise<string> {
  return new Promise((resolve) => {
    const backdrop = document.createElement("div");
    backdrop.className = "dialog-backdrop";
    const box = document.createElement("div");
    box.className = "dialog";
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-modal", "true");
    const h = document.createElement("h2");
    h.textContent = opts.title;
    box.appendChild(h);
    for (const line of opts.lines) {
      const p = document.createElement("p");
      p.textContent = line;
      box.appendChild(p);
    }
    if (opts.items?.length) {
      const ul = document.createElement("ul");
      for (const item of opts.items) {
        const li = document.createElement("li");
        li.textContent = item;
        ul.appendChild(li);
      }
      box.appendChild(ul);
    }
    const row = document.createElement("div");
    row.className = "dialog-buttons";
    const buttons: HTMLButtonElement[] = [];
    for (const b of opts.buttons) {
      const el = document.createElement("button");
      el.textContent = b.label;
      el.dataset.choice = b.id;
      if (b.kind) el.className = b.kind;
      row.appendChild(el);
      buttons.push(el);
    }
    box.appendChild(row);
    backdrop.appendChild(box);
    const previous = document.activeElement as HTMLElement | null;
    const done = (id: string) => {
      open--;
      backdrop.remove();
      previous?.focus?.();
      resolve(id);
    };
    row.addEventListener("click", (e) => {
      const id = (e.target as HTMLElement).closest("button")?.dataset.choice;
      if (id) done(id);
    });
    backdrop.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        done(opts.cancelId);
      } else if (e.key === "Tab" && buttons.length) {
        e.preventDefault();
        const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[(i + (e.shiftKey ? buttons.length - 1 : 1)) % buttons.length].focus();
      }
      e.stopPropagation();
    });
    open++;
    document.body.appendChild(backdrop);
    (buttons.find((b) => b.classList.contains("primary")) ?? buttons[buttons.length - 1])?.focus();
  });
}
