import type { ReactNode } from "react";

interface MessageTextProps {
  text: string;
}

interface ListEntry {
  indent: number;
  ordered: boolean;
  indexLabel?: string;
  content: string;
}

function getFileBadge(label: string, target: string): { tag: string; tone: string } | null {
  if (!target.startsWith("file://") && !/\.[a-zA-Z0-9]{1,8}(?:#L\d+)?$/.test(label)) {
    return null;
  }
  const clean = label.split("#")[0].toLowerCase();
  const ext = clean.includes(".") ? clean.split(".").pop() ?? "" : "";
  switch (ext) {
    case "rs":
      return { tag: "RS", tone: "rust" };
    case "tsx":
      return { tag: "TSX", tone: "react" };
    case "ts":
    case "mts":
      return { tag: "TS", tone: "ts" };
    case "js":
    case "mjs":
    case "jsx":
      return { tag: "JS", tone: "js" };
    case "css":
    case "scss":
      return { tag: "{}", tone: "css" };
    case "json":
      return { tag: "JSON", tone: "json" };
    case "md":
      return { tag: "MD", tone: "md" };
    case "toml":
    case "yaml":
    case "yml":
      return { tag: "CFG", tone: "cfg" };
    default:
      return { tag: "FILE", tone: "neutral" };
  }
}

function renderInline(line: string, keyPrefix: string): ReactNode[] {
  // Split inline code (`...`), bold (**...**), italic (*...*), and markdown links [label](url)
  const tokenRegex = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*\n]+\*|\[[^\]]+\]\([^)]+\))/g;
  const parts: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let idx = 0;

  while ((match = tokenRegex.exec(line)) !== null) {
    if (match.index > lastIndex) {
      parts.push(line.slice(lastIndex, match.index));
    }
    const token = match[0];
    if (token.startsWith("`") && token.endsWith("`")) {
      parts.push(
        <code key={`${keyPrefix}-c-${idx}`} className="msg-inline-code">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith("**") && token.endsWith("**")) {
      parts.push(
        <strong key={`${keyPrefix}-b-${idx}`}>{token.slice(2, -2)}</strong>,
      );
    } else if (token.startsWith("*") && token.endsWith("*")) {
      parts.push(
        <em key={`${keyPrefix}-i-${idx}`}>{token.slice(1, -1)}</em>,
      );
    } else if (token.startsWith("[")) {
      const m = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (m) {
        const badge = getFileBadge(m[1], m[2]);
        if (badge) {
          parts.push(
            <span key={`${keyPrefix}-f-${idx}`} className="msg-file-chip" title={m[2]}>
              <span className={`msg-file-badge ${badge.tone}`}>{badge.tag}</span>
              <span className="msg-file-name">{m[1]}</span>
            </span>,
          );
        } else {
          parts.push(
            <span key={`${keyPrefix}-l-${idx}`} className="msg-link-label">
              {m[1]}
            </span>,
          );
        }
      } else {
        parts.push(token);
      }
    }
    lastIndex = match.index + token.length;
    idx += 1;
  }

  if (lastIndex < line.length) {
    parts.push(line.slice(lastIndex));
  }
  return parts;
}

export function MessageText({ text }: MessageTextProps) {
  const blocks: ReactNode[] = [];
  const segments = text.split(/```/);

  segments.forEach((segment, segIdx) => {
    if (segIdx % 2 === 1) {
      const firstNewline = segment.indexOf("\n");
      const lang = firstNewline >= 0 ? segment.slice(0, firstNewline).trim() : "";
      const codeBody =
        firstNewline >= 0 ? segment.slice(firstNewline + 1) : segment;
      blocks.push(
        <div key={`code-${segIdx}`} className="msg-code-wrap">
          {lang && <div className="msg-code-lang">{lang}</div>}
          <pre className="msg-code-block">
            <code>{codeBody.trimEnd()}</code>
          </pre>
        </div>,
      );
      return;
    }

    const lines = segment.split("\n");
    let listItems: ListEntry[] = [];

    const flushList = (keySuffix: number) => {
      if (listItems.length === 0) return;
      const baseIndent = Math.min(...listItems.map((entry) => entry.indent));
      blocks.push(
        <ul key={`ul-${segIdx}-${keySuffix}`} className="msg-list">
          {listItems.map((entry, i) => {
            const level = entry.indent > baseIndent ? Math.min(2, Math.floor((entry.indent - baseIndent) / 2)) : 0;
            return (
              <li
                key={i}
                className={`msg-list-item level-${level} ${entry.ordered ? "ordered" : ""}`}
                data-order={entry.indexLabel}
              >
                {renderInline(entry.content, `li-${segIdx}-${keySuffix}-${i}`)}
              </li>
            );
          })}
        </ul>,
      );
      listItems = [];
    };

    lines.forEach((rawLine, lineIdx) => {
      const trimmed = rawLine.trim();
      if (!trimmed) {
        flushList(lineIdx);
        return;
      }

      const leadingSpaces = rawLine.search(/\S/);
      const indent = leadingSpaces >= 0 ? leadingSpaces : 0;

      const bulletMatch = /^[-*•]\s+(.*)$/.exec(trimmed);
      if (bulletMatch) {
        listItems.push({ indent, ordered: false, content: bulletMatch[1] });
        return;
      }

      const numberedMatch = /^(\d+\.)\s+(.*)$/.exec(trimmed);
      if (numberedMatch) {
        listItems.push({
          indent,
          ordered: true,
          indexLabel: numberedMatch[1],
          content: numberedMatch[2],
        });
        return;
      }

      flushList(lineIdx);

      const headingMatch = /^(#{1,4})\s+(.*)$/.exec(trimmed);
      if (headingMatch) {
        const level = headingMatch[1].length;
        blocks.push(
          <div key={`h-${segIdx}-${lineIdx}`} className={`msg-heading level-${level}`}>
            {renderInline(headingMatch[2], `h-${segIdx}-${lineIdx}`)}
          </div>,
        );
        return;
      }

      if (trimmed.startsWith(">")) {
        blocks.push(
          <blockquote key={`bq-${segIdx}-${lineIdx}`} className="msg-blockquote">
            {renderInline(trimmed.replace(/^>\s?/, ""), `bq-${segIdx}-${lineIdx}`)}
          </blockquote>,
        );
        return;
      }

      blocks.push(
        <p key={`p-${segIdx}-${lineIdx}`} className="msg-para">
          {renderInline(trimmed, `p-${segIdx}-${lineIdx}`)}
        </p>,
      );
    });

    flushList(lines.length);
  });

  return <div className="msg-body">{blocks}</div>;
}
