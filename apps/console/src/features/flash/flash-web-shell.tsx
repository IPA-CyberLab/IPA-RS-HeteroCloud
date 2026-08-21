import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import "./flash-web-shell.css";

export type FlashShellConnectionState =
  | "connecting"
  | "connected"
  | "closed"
  | "error";

export function FlashWebShell({
  url,
  onStateChange,
}: {
  url: string;
  onStateChange: (state: FlashShellConnectionState) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: true,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 14,
      scrollback: 5_000,
      theme: {
        background: "#111418",
        foreground: "#f2f3f3",
        cursor: "#22d3a7",
        selectionBackground: "#356d9b88",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    const socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();

    const resize = () => {
      fit.fit();
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(
          JSON.stringify({
            type: "resize",
            cols: terminal.cols,
            rows: terminal.rows,
          }),
        );
      }
    };
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(container);
    const input = terminal.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) socket.send(encoder.encode(data));
    });

    onStateChange("connecting");
    socket.addEventListener("open", () => {
      onStateChange("connected");
      resize();
      terminal.focus();
    });
    socket.addEventListener("message", (event) => {
      if (event.data instanceof ArrayBuffer) {
        terminal.write(decoder.decode(event.data, { stream: true }));
      } else if (event.data instanceof Blob) {
        void event.data.arrayBuffer().then((buffer) => terminal.write(decoder.decode(buffer)));
      } else if (typeof event.data === "string") {
        try {
          const value = JSON.parse(event.data) as { type?: unknown; message?: unknown };
          if (value.type === "error" && typeof value.message === "string") {
            terminal.writeln(`\r\n\x1b[31m${value.message}\x1b[0m`);
            onStateChange("error");
            return;
          }
        } catch {
          // Non-control text is terminal output.
        }
        terminal.write(event.data);
      }
    });
    socket.addEventListener("error", () => onStateChange("error"));
    socket.addEventListener("close", () => onStateChange("closed"));

    return () => {
      input.dispose();
      resizeObserver.disconnect();
      socket.close();
      terminal.dispose();
    };
  }, [onStateChange, url]);

  return <div className="flash-web-shell" ref={containerRef} />;
}
