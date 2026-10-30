// Pointer-driven reordering of a container's children. A mouse drag starts after a
// small move; touch waits for a long press so the page can still scroll.
const MOVE_THRESHOLD = 5;
const LONG_PRESS_MS = 350;

const contains = (rect, x, y) => x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;

export function makeSortable(container, { itemSelector, ignoreSelector, onSort }) {
  let drag = null;
  let suppressClick = false;

  function follow() {
    const { item } = drag;
    item.style.transform = "";
    const rect = item.getBoundingClientRect();
    item.style.transform = `translate(${drag.x - drag.grabX - rect.left}px, ${drag.y - drag.grabY - rect.top}px) scale(1.03)`;
  }

  function begin() {
    clearTimeout(drag.timer);
    drag.active = true;
    drag.before = [...container.children];
    drag.item.setPointerCapture?.(drag.pointerId);
    drag.item.classList.add("dragging");
    container.classList.add("sorting");
    navigator.vibrate?.(10);
    follow();
  }

  function reorder() {
    const children = [...container.children];
    // Tiles still sliding into place would otherwise swap straight back under the pointer.
    const target = children.find((child) => child !== drag.item && child.matches(itemSelector)
      && !child.getAnimations().length && contains(child.getBoundingClientRect(), drag.x, drag.y));
    if (!target) return;
    const first = new Map(children.map((child) => [child, child.getBoundingClientRect()]));
    const backward = children.indexOf(target) < children.indexOf(drag.item);
    container.insertBefore(drag.item, backward ? target : target.nextSibling);
    for (const child of children) {
      if (child === drag.item) continue;
      const was = first.get(child), now = child.getBoundingClientRect();
      if (was.left === now.left && was.top === now.top) continue;
      child.animate([{ transform: `translate(${was.left - now.left}px, ${was.top - now.top}px)` }, { transform: "none" }], { duration: 160, easing: "ease-out" });
    }
  }

  function end() {
    if (!drag) return;
    clearTimeout(drag.timer);
    const { item, active, before } = drag;
    drag = null;
    if (!active) return;
    suppressClick = true;
    item.classList.remove("dragging");
    item.style.transform = "";
    container.classList.remove("sorting");
    const after = [...container.children];
    // A re-render during the drag replaced the tiles, so there is nothing to commit.
    if (item.isConnected && after.some((child, index) => child !== before[index])) onSort(after);
  }

  container.addEventListener("pointerdown", (event) => {
    suppressClick = false;
    const item = event.target.closest(itemSelector);
    if (!item || event.button !== 0 || event.target.closest(ignoreSelector)) return;
    const rect = item.getBoundingClientRect();
    const touch = event.pointerType === "touch";
    drag = {
      item, touch, pointerId: event.pointerId, active: false,
      startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY,
      grabX: event.clientX - rect.left, grabY: event.clientY - rect.top,
      timer: touch ? setTimeout(begin, LONG_PRESS_MS) : null,
    };
  });
  document.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    drag.x = event.clientX;
    drag.y = event.clientY;
    if (!drag.active) {
      if (Math.hypot(drag.x - drag.startX, drag.y - drag.startY) <= MOVE_THRESHOLD) return;
      if (drag.touch) { clearTimeout(drag.timer); drag = null; } else begin();
      return;
    }
    follow();
    reorder();
    follow();
  });
  document.addEventListener("pointerup", end);
  document.addEventListener("pointercancel", end);
  // Once a touch drag starts, the page must not scroll underneath it.
  document.addEventListener("touchmove", (event) => { if (drag?.active) event.preventDefault(); }, { passive: false });
  container.addEventListener("contextmenu", (event) => { if (drag) event.preventDefault(); });
  // The release that ends a drag must not also open the tile.
  container.addEventListener("click", (event) => {
    if (!suppressClick) return;
    suppressClick = false;
    event.preventDefault();
    event.stopPropagation();
  }, true);
}
