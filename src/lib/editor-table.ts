// Düzenleyicideki tablo işlemleri (satır/sütun ekleme-silme, stil, genişlik). Stiller satır içine
// yazılır: alıcıda stil sayfası olmadığı için tablo yalnızca böyle aynı görünür.
// Birleştirilmiş hücreli (colspan/rowspan) tablolarda sütun işlemleri hücre sırasına göre yapılır.
import { TABLE_STYLES, type TableOp } from "./editor";

function emptyCell(like: HTMLTableCellElement): HTMLTableCellElement {
  const c = like.cloneNode(false) as HTMLTableCellElement;
  for (const a of ["colspan", "rowspan", "id"]) c.removeAttribute(a);
  c.innerHTML = "<br>";
  return c;
}

/** Tablonun stilini (TABLE_STYLES anahtarı) hücrelerin satır içi stiline yazar */
export function applyTableStyle(table: HTMLTableElement, key: string) {
  const s = TABLE_STYLES.find((x) => x.key === key);
  if (!s) return;
  table.dataset.lynStyle = key;
  table.style.borderCollapse = "collapse";
  table.style.border = s.lines === "all" ? `1px solid ${s.line}` : "none";
  Array.from(table.rows).forEach((row, i) => {
    const head = !!s.head && i === 0;
    for (const c of row.cells) {
      c.style.border = s.lines === "all" ? `1px solid ${s.line}` : "none";
      if (s.lines === "rows") {
        c.style.borderBottom = `${head ? 2 : 1}px solid ${s.line}`;
        if (i === 0) c.style.borderTop = `1px solid ${s.line}`;
      }
      c.style.backgroundColor = head ? s.head!.bg : s.band && i % 2 === 1 ? s.band : "";
      c.style.color = head ? s.head!.fg : "";
      c.style.fontWeight = head ? "bold" : "";
    }
  });
}

/** Satır/sütun değişince başlık ve şerit renkleri yeni sıraya göre yeniden yazılır */
function restyle(table: HTMLTableElement) {
  const s = TABLE_STYLES.find((x) => x.key === table.dataset.lynStyle);
  if (s && (s.head || s.band)) applyTableStyle(table, s.key);
}

/** İşlemi uygular; imlecin gideceği hücreyi döndürür (tablo silindiyse null) */
export function tableOp(cell: HTMLTableCellElement, op: TableOp, arg = ""): HTMLTableCellElement | null {
  const row = cell.parentElement as HTMLTableRowElement;
  const table = cell.closest("table") as HTMLTableElement;
  const rows = Array.from(table.rows);
  const col = cell.cellIndex;
  const at = (r: HTMLTableRowElement, i: number) => r.cells[Math.max(0, Math.min(i, r.cells.length - 1))];
  switch (op) {
    case "rowAbove":
    case "rowBelow": {
      const fresh = row.cloneNode(false) as HTMLTableRowElement;
      for (const c of row.cells) fresh.appendChild(emptyCell(c));
      if (op === "rowAbove") row.before(fresh);
      else row.after(fresh);
      restyle(table);
      return at(fresh, col);
    }
    case "colLeft":
    case "colRight":
      for (const r of rows) {
        const ref = at(r, col);
        if (!ref) continue;
        if (op === "colLeft") ref.before(emptyCell(ref));
        else ref.after(emptyCell(ref));
      }
      restyle(table);
      return at(row, op === "colLeft" ? col : col + 1);
    case "delRow": {
      const next = rows[rows.indexOf(row) + 1] ?? rows[rows.indexOf(row) - 1];
      if (!next) break;
      row.remove();
      restyle(table);
      return at(next, col);
    }
    case "delCol":
      if (row.cells.length <= 1) break;
      for (const r of rows) r.cells[col]?.remove();
      return at(row, col);
    case "style":
      applyTableStyle(table, arg);
      return cell;
    case "fit":
      table.style.width = "100%";
      return cell;
    case "auto":
      table.style.width = "";
      table.removeAttribute("width");
      for (const r of rows) for (const c of r.cells) {
        c.style.width = "";
        c.removeAttribute("width");
      }
      return cell;
    case "shade":
      cell.style.backgroundColor = arg;
      return cell;
    case "delTable":
      break;
  }
  // Son satır/sütun silinince ya da "Tabloyu Sil": tablo kalkar
  table.remove();
  return null;
}
