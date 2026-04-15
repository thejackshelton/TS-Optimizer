import { extname } from 'node:path';
import {
  parse,
  type Comment,
  type Diagnostic,
  type ParseOptions as YukuParseOptions,
  type ParseResult as YukuParseResult,
} from 'yuku-parser';
import type { Program } from '@oxc-project/types';

export interface ParserOptions extends YukuParseOptions {
  experimentalRawTransfer?: boolean;
}

export interface ParseError {
  message: string;
  start: number;
  end: number;
}

export interface EcmaScriptModule {
  staticImports?: Array<{
    moduleRequest: { value: string };
    entries?: Array<{
      localName: { value: string };
      importName: {
        kind: 'Default' | 'NamespaceObject' | string;
        name?: string;
      };
    }>;
  }>;
  staticExports?: Array<{
    entries?: Array<{
      exportName: {
        kind: 'Default' | string;
        name?: string;
      };
    }>;
  }>;
}

export interface ParseResult {
  program: Program;
  comments: Comment[];
  errors: ParseError[];
  diagnostics: Diagnostic[];
  module: EcmaScriptModule | undefined;
}

export function parseSync(
  filename: string,
  sourceText: string,
  options?: ParserOptions,
): ParseResult {
  const result = parse(sourceText, buildParseOptions(filename, options));
  const errors = result.diagnostics
    .filter((diagnostic) => diagnostic.severity === 'error')
    .map((diagnostic) => ({
      message: diagnostic.message,
      start: diagnostic.start,
      end: diagnostic.end,
    }));

  const module = buildModuleInfo(result.program as Program);

  return {
    program: result.program as Program,
    comments: result.comments,
    errors,
    diagnostics: result.diagnostics,
    module,
  };
}

function buildModuleInfo(program: Program): EcmaScriptModule | undefined {
  const staticImports: NonNullable<EcmaScriptModule['staticImports']> = [];
  const staticExports: NonNullable<EcmaScriptModule['staticExports']> = [];

  for (const stmt of program.body ?? []) {
    if (stmt.type === 'ImportDeclaration') {
      staticImports.push({
        moduleRequest: { value: stmt.source.value },
        entries: (stmt.specifiers ?? []).map((spec: any) => ({
          localName: { value: spec.local.name },
          importName:
            spec.type === 'ImportDefaultSpecifier'
              ? { kind: 'Default' }
              : spec.type === 'ImportNamespaceSpecifier'
                ? { kind: 'NamespaceObject' }
                : { kind: 'Named', name: spec.imported?.name ?? spec.local.name },
        })),
      });
      continue;
    }

    if (stmt.type === 'ExportNamedDeclaration') {
      const entries: Array<{ exportName: { kind: 'Default' | string; name?: string } }> = [];

      if (stmt.declaration?.type === 'VariableDeclaration') {
        for (const decl of stmt.declaration.declarations ?? []) {
          if (decl.id?.type === 'Identifier') {
            entries.push({ exportName: { kind: 'Named', name: decl.id.name } });
          }
        }
      } else if (
        (stmt.declaration?.type === 'FunctionDeclaration' ||
          stmt.declaration?.type === 'ClassDeclaration') &&
        stmt.declaration.id?.name
      ) {
        entries.push({ exportName: { kind: 'Named', name: stmt.declaration.id.name } });
      }

      for (const spec of stmt.specifiers ?? []) {
        entries.push({
          exportName:
            spec.exported?.name === 'default'
              ? { kind: 'Default' }
              : { kind: 'Named', name: spec.exported?.name },
        });
      }

      staticExports.push({ entries });
      continue;
    }

    if (stmt.type === 'ExportDefaultDeclaration') {
      staticExports.push({ entries: [{ exportName: { kind: 'Default' } }] });
    }
  }

  if (staticImports.length === 0 && staticExports.length === 0) {
    return undefined;
  }

  return {
    staticImports: staticImports.length > 0 ? staticImports : undefined,
    staticExports: staticExports.length > 0 ? staticExports : undefined,
  };
}

function buildParseOptions(
  filename: string,
  options?: ParserOptions,
): YukuParseOptions {
  const lang = options?.lang ?? detectLang(filename);

  return {
    sourceType: options?.sourceType ?? 'module',
    lang,
    preserveParens: options?.preserveParens ?? true,
    allowReturnOutsideFunction: options?.allowReturnOutsideFunction ?? false,
    semanticErrors: options?.semanticErrors ?? false,
  };
}

function detectLang(filename: string): YukuParseOptions['lang'] {
  switch (extname(filename).toLowerCase()) {
    case '.tsx':
      return 'tsx';
    case '.jsx':
      return 'jsx';
    case '.ts':
      return 'ts';
    default:
      return 'js';
  }
}
