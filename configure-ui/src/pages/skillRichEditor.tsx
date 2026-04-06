import { useMemo } from "react";
import Box from "@mui/material/Box";
import {
  MDXEditor,
  BlockTypeSelect,
  BoldItalicUnderlineToggles,
  codeBlockPlugin,
  codeMirrorPlugin,
  CreateLink,
  headingsPlugin,
  InsertCodeBlock,
  InsertTable,
  InsertThematicBreak,
  linkPlugin,
  listsPlugin,
  ListsToggle,
  markdownShortcutPlugin,
  quotePlugin,
  Separator,
  tablePlugin,
  thematicBreakPlugin,
  toolbarPlugin,
  UndoRedo,
  CodeToggle,
} from "@mdxeditor/editor";
import "@mdxeditor/editor/style.css";

const CODE_LANG: Record<string, string> = {
  text: "Plain Text",
  markdown: "Markdown",
  md: "Markdown",
  json: "JSON",
  yaml: "YAML",
  rust: "Rust",
  javascript: "JavaScript",
  js: "JavaScript",
  typescript: "TypeScript",
  ts: "TypeScript",
  bash: "Bash",
  sh: "Shell",
  python: "Python",
  py: "Python",
};

export function SkillRichEditor({
  markdown,
  onChange,
  themeMode,
}: {
  markdown: string;
  /** 第二项为 true 表示 MDX 对初始文档规范化（非用户编辑），用于与「初始快照」对齐。 */
  onChange: (markdown: string, initialMarkdownNormalize?: boolean) => void;
  themeMode: "light" | "dark";
}) {
  const plugins = useMemo(
    () => [
      headingsPlugin(),
      listsPlugin(),
      quotePlugin(),
      thematicBreakPlugin(),
      markdownShortcutPlugin(),
      linkPlugin(),
      tablePlugin(),
      /**
       * 无 ``` 语言标记时 mdast.lang 为空串，CodeMirror 需能匹配；空语言回退到 `text`。
       * @see CodeBlockNode: match(language) 失败时用 defaultCodeBlockLanguage 再匹配一次。
       */
      codeBlockPlugin({ defaultCodeBlockLanguage: "text" }),
      codeMirrorPlugin({
        codeBlockLanguages: CODE_LANG,
      }),
      toolbarPlugin({
        toolbarContents: () => (
          <>
            <UndoRedo />
            <Separator />
            <BoldItalicUnderlineToggles />
            <CodeToggle />
            <Separator />
            <ListsToggle />
            <Separator />
            <BlockTypeSelect />
            <Separator />
            <CreateLink />
            <InsertThematicBreak />
            <InsertCodeBlock />
            <InsertTable />
          </>
        ),
      }),
    ],
    [],
  );

  const themeClass = themeMode === "dark" ? "dark-theme" : "light-theme";

  return (
    <Box
      className={`skills-rich-md-wrap ${themeClass}`}
      sx={{
        width: "100%",
        minHeight: 0,
        borderRadius: "var(--radius-control)",
      }}
    >
      <MDXEditor
        markdown={markdown}
        onChange={(md, initialMarkdownNormalize) =>
          onChange(md, initialMarkdownNormalize)
        }
        plugins={plugins}
        autoFocus
        spellCheck={false}
        contentEditableClassName="skill-mdx-editable"
      />
    </Box>
  );
}
