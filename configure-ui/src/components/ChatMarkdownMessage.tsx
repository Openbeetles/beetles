import { Fragment, useMemo, type ReactNode } from "react";
import type { Nodes, Table, TableRow } from "mdast";
import {
  isSafeMarkdownUrl,
  normalizeMarkdownText,
  parseChatMarkdown,
} from "./ChatMarkdownMessageModel";
import "./ChatMarkdownMessage.css";

const HEADING_TAGS = ["h1", "h2", "h3", "h4", "h5", "h6"] as const;

function renderChildren(node: Nodes & { children?: Nodes[] }, keyPrefix: string): ReactNode {
  return node.children?.map((child, index) =>
    renderNode(child, `${keyPrefix}-${index}`),
  );
}

function renderTableRow(
  row: TableRow,
  key: string,
  header: boolean,
  align: Table["align"] = [],
): ReactNode {
  const Cell = header ? "th" : "td";
  const tableAlign = align ?? [];
  return (
    <tr key={key}>
      {row.children.map((cell, index) => (
        <Cell key={`${key}-${index}`} style={{ textAlign: tableAlign[index] ?? undefined }}>
          {renderChildren(cell, `${key}-${index}`)}
        </Cell>
      ))}
    </tr>
  );
}

function renderTable(node: Table, key: string): ReactNode {
  const [head, ...body] = node.children;
  return (
    <div key={key} className="chat-markdown-message__table-wrap">
      <table>
        {head ? <thead>{renderTableRow(head, `${key}-head`, true, node.align)}</thead> : null}
        {body.length > 0 ? (
          <tbody>
            {body.map((row, index) =>
              renderTableRow(row, `${key}-body-${index}`, false, node.align),
            )}
          </tbody>
        ) : null}
      </table>
    </div>
  );
}

function renderNode(node: Nodes, key: string): ReactNode {
  switch (node.type) {
    case "root":
      return <Fragment key={key}>{renderChildren(node, key)}</Fragment>;
    case "paragraph":
      return <p key={key}>{renderChildren(node, key)}</p>;
    case "heading": {
      const Tag = HEADING_TAGS[Math.min(Math.max(node.depth, 1), 6) - 1];
      return <Tag key={key}>{renderChildren(node, key)}</Tag>;
    }
    case "text":
      return node.value;
    case "break":
      return <br key={key} />;
    case "emphasis":
      return <em key={key}>{renderChildren(node, key)}</em>;
    case "strong":
      return <strong key={key}>{renderChildren(node, key)}</strong>;
    case "delete":
      return <del key={key}>{renderChildren(node, key)}</del>;
    case "inlineCode":
      return (
        <code key={key} className="chat-markdown-message__inline-code">
          {node.value}
        </code>
      );
    case "code":
      return (
        <pre key={key}>
          <code>{node.value}</code>
        </pre>
      );
    case "blockquote":
      return <blockquote key={key}>{renderChildren(node, key)}</blockquote>;
    case "list": {
      const ListTag = node.ordered ? "ol" : "ul";
      return (
        <ListTag key={key} start={node.start ?? undefined}>
          {renderChildren(node, key)}
        </ListTag>
      );
    }
    case "listItem": {
      const isTask = typeof node.checked === "boolean";
      return (
        <li
          key={key}
          className={isTask ? "chat-markdown-message__task-item" : undefined}
        >
          {isTask ? (
            <input type="checkbox" checked={node.checked ?? false} readOnly tabIndex={-1} />
          ) : null}
          {renderChildren(node, key)}
        </li>
      );
    }
    case "link":
      if (!isSafeMarkdownUrl(node.url)) {
        return <Fragment key={key}>{renderChildren(node, key)}</Fragment>;
      }
      return (
        <a key={key} href={node.url} target="_blank" rel="noreferrer">
          {renderChildren(node, key)}
        </a>
      );
    case "image":
      return node.alt ? <span key={key}>{node.alt}</span> : null;
    case "thematicBreak":
      return <hr key={key} />;
    case "table":
      return renderTable(node, key);
    case "html":
      return node.value;
    default:
      if ("children" in node) {
        return <Fragment key={key}>{renderChildren(node, key)}</Fragment>;
      }
      if ("value" in node && typeof node.value === "string") {
        return node.value;
      }
      return null;
  }
}

export function ChatMarkdownMessage({ markdown }: { markdown: string }) {
  const normalized = normalizeMarkdownText(markdown);
  const rendered = useMemo(() => {
    try {
      return renderNode(parseChatMarkdown(normalized), "root");
    } catch {
      return normalized;
    }
  }, [normalized]);

  return <div className="chat-markdown-message">{rendered}</div>;
}
