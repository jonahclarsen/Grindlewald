const isColor = ({ command }) => command === "color" || command === "white";

export function createControlQueue(send, { onStart, onSuccess, onError } = {}) {
  const pending = [];
  let sending = false;
  let generation = 0;

  return {
    clear() {
      pending.length = 0;
      generation += 1;
    },
    async enqueue(command) {
      const previous = pending.at(-1);
      // Combine slider updates only for the same target, without crossing
      // intervening commands such as power or preset changes.
      if (previous && previous.device === command.device) {
        if (command.command === "brightness" && isColor(previous)) {
          command = { ...previous, brightness: command.value };
          pending.pop();
        } else if (
          (isColor(command) && (isColor(previous) || previous.command === "brightness")) ||
          (command.command === "brightness" && previous.command === "brightness") ||
          (command.command === "seek_breathing" && previous.command === "seek_breathing")
        ) {
          if (isColor(command) && command.brightness == null) {
            command = { ...command, brightness: previous.command === "brightness" ? previous.value : previous.brightness };
          }
          pending.pop();
        }
      }
      pending.push(command);
      if (sending) return;
      sending = true;
      try {
        onStart?.();
        while (pending.length) {
          const currentGeneration = generation;
          const latest = pending.shift();
          try {
            const message = await send(latest);
            if (currentGeneration === generation) onSuccess?.(message);
          } catch (error) {
            if (currentGeneration === generation) onError?.(error);
          }
        }
      } finally {
        sending = false;
      }
    },
  };
}
