import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const MAX_FRAME_BYTES = 64 * 1024;

function getRunDir() {
  if (process.platform === "darwin") {
    return path.join(
      os.homedir(),
      "Library",
      "Application Support",
      "com.antigravity.floating-dock",
      "run",
    );
  }
  return path.join(os.homedir(), ".config", "com.antigravity.floating-dock", "run");
}

async function loadSidecarSdk() {
  const candidates = [
    () => import("sidecar_sdk"),
    () =>
      import(
        pathToFileURL(
          path.join(
            os.homedir(),
            ".gemini",
            "antigravity",
            "lib",
            "node_modules",
            "sidecar_sdk",
            "index.mjs",
          ),
        ).href
      ),
    () =>
      import(
        pathToFileURL(
          path.join(
            os.homedir(),
            ".gemini",
            "config",
            "node_modules",
            "sidecar_sdk",
            "index.mjs",
          ),
        ).href
      ),
  ];

  for (const loader of candidates) {
    try {
      const mod = await loader();
      if (mod?.SidecarApp) return mod.SidecarApp;
    } catch {
      // Try next candidate
    }
  }
  return null;
}

function timingSafeEqualStr(a, b) {
  if (typeof a !== "string" || typeof b !== "string") return false;
  const bufA = Buffer.from(a, "utf8");
  const bufB = Buffer.from(b, "utf8");
  if (bufA.length !== bufB.length) return false;
  return crypto.timingSafeEqual(bufA, bufB);
}

function listConversationsFromLs(pageSize = 25) {
  return new Promise((resolve, reject) => {
    const lsAddr = process.env.ANTIGRAVITY_LS_ADDRESS;
    const csrf = process.env.ANTIGRAVITY_CSRF_TOKEN;
    if (!lsAddr || !csrf) {
      reject(new Error("Missing ANTIGRAVITY_LS_ADDRESS or ANTIGRAVITY_CSRF_TOKEN"));
      return;
    }
    const [host, portStr] = lsAddr.split(":");
    const body = JSON.stringify({ pageSize, includeArchived: false });
    const req = http.request(
      {
        hostname: host || "127.0.0.1",
        port: Number(portStr),
        path: "/product.v1.ConversationService/ListConversations",
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Connect-Protocol-Version": "1",
          "x-antigravity-csrf-token": csrf,
          "Content-Length": Buffer.byteLength(body, "utf8"),
        },
        timeout: 1500,
      },
      (res) => {
        let raw = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          raw += chunk;
        });
        res.on("end", () => {
          if (res.statusCode !== 200) {
            reject(new Error(`ListConversations HTTP ${res.statusCode}`));
            return;
          }
          try {
            const parsed = JSON.parse(raw || "{}");
            const items = Array.isArray(parsed.conversations) ? parsed.conversations : [];
            const mapped = items.map((c) => ({
              id: String(c?.id || ""),
              title: String(
                c?.annotations?.title || c?.state?.summary || (c?.id ? c.id.slice(0, 8) : "Untitled"),
              ),
              runState: String(c?.state?.runState || "RUN_STATE_IDLE"),
              updateTime: String(c?.state?.updateTime || ""),
              projectId: c?.metadata?.projectId ? String(c.metadata.projectId) : null,
            })).filter((c) => c.id.length > 0);
            resolve(mapped);
          } catch (err) {
            reject(err);
          }
        });
      },
    );
    req.on("timeout", () => {
      req.destroy(new Error("ListConversations timeout"));
    });
    req.on("error", (err) => reject(err));
    req.write(body);
    req.end();
  });
}

async function main() {
  const SidecarApp = await loadSidecarSdk();
  if (!SidecarApp) {
    console.error("[dock-sidecar] sidecar_sdk not found");
    process.exit(1);
  }

  let app;
  try {
    app = new SidecarApp();
  } catch (err) {
    console.error("[dock-sidecar] not running inside Antigravity sidecar env:", err?.message || err);
    process.exit(1);
  }

  // Minimal page so Antigravity sidecar web port stays cleanly bound.
  app.page("/", () => "<!doctype html><html><body>Antigravity Floating Dock Bridge</body></html>");
  await app.run();

  const runDir = getRunDir();
  fs.mkdirSync(runDir, { recursive: true, mode: 0o700 });
  try {
    fs.chmodSync(runDir, 0o700);
  } catch {}

  const sockPath = path.join(runDir, "sidecar-bridge.sock");
  const tokenPath = path.join(runDir, "sidecar-bridge.token");

  try {
    if (fs.existsSync(sockPath)) fs.unlinkSync(sockPath);
  } catch {}

  const token = crypto.randomBytes(32).toString("hex");
  fs.writeFileSync(tokenPath, `${token}\n`, { mode: 0o600 });
  try {
    fs.chmodSync(tokenPath, 0o600);
  } catch {}

  const server = net.createServer((socket) => {
    let buffer = "";
    let finished = false;

    const reply = (obj) => {
      if (finished) return;
      finished = true;
      try {
        socket.end(`${JSON.stringify(obj)}\n`);
      } catch {}
    };

    socket.setTimeout(15000, () => reply({ ok: false, error: "timeout" }));

    socket.on("data", (chunk) => {
      if (finished) return;
      buffer += chunk.toString("utf8");
      if (Buffer.byteLength(buffer, "utf8") > MAX_FRAME_BYTES) {
        reply({ ok: false, error: "frame_too_large" });
        return;
      }

      const nl = buffer.indexOf("\n");
      if (nl === -1) return;

      const line = buffer.slice(0, nl).trim();
      let req;
      try {
        req = JSON.parse(line);
      } catch {
        reply({ ok: false, error: "malformed_json" });
        return;
      }

      if (!timingSafeEqualStr(req?.token, token)) {
        reply({ ok: false, error: "unauthorized" });
        return;
      }

      const action = String(req?.action || "");
      if (action === "ping") {
        reply({ ok: true, config: app.config });
        return;
      }

      if (action === "list_conversations") {
        void listConversationsFromLs(25)
          .then((conversations) => reply({ ok: true, conversations }))
          .catch((err) => reply({ ok: false, error: err?.message || String(err) }));
        return;
      }

      if (action === "send_message") {
        const cid = String(req?.conversationId || "").trim();
        const message = String(req?.message || "");
        if (!cid) {
          reply({ ok: false, error: "missing_conversation_id" });
          return;
        }
        if (!message.trim()) {
          reply({ ok: false, error: "empty_message" });
          return;
        }
        try {
          const result = app.sendMessage(cid, message);
          reply({ ok: true, conversationId: cid, result });
        } catch (err) {
          reply({ ok: false, error: err?.message || String(err) });
        }
        return;
      }

      if (action === "new_conversation") {
        const message = String(req?.message || "");
        const model = req?.model ? String(req.model).trim() : "";
        if (!message.trim()) {
          reply({ ok: false, error: "empty_message" });
          return;
        }
        try {
          const result = app.startConversation(
            message,
            model ? { model } : {},
          );
          const cid =
            result?.conversationId || result?.conversation_id || result?.id || null;
          reply({
            ok: true,
            conversationId: cid ? String(cid) : null,
            result,
          });
        } catch (err) {
          reply({ ok: false, error: err?.message || String(err) });
        }
        return;
      }

      reply({ ok: false, error: "unknown_action" });
    });

    socket.on("error", () => {});
  });

  server.listen(sockPath, () => {
    try {
      fs.chmodSync(sockPath, 0o600);
    } catch {}
    console.log("[dock-sidecar] unix bridge listening");
  });

  const cleanup = async () => {
    try {
      server.close();
    } catch {}
    try {
      if (fs.existsSync(sockPath)) fs.unlinkSync(sockPath);
    } catch {}
    try {
      if (fs.existsSync(tokenPath)) fs.unlinkSync(tokenPath);
    } catch {}
    try {
      await app.close();
    } catch {}
    process.exit(0);
  };

  process.on("SIGTERM", () => void cleanup());
  process.on("SIGINT", () => void cleanup());
}

main().catch((err) => {
  console.error("[dock-sidecar] fatal:", err?.message || err);
  process.exit(1);
});
