import ts from 'typescript';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const packageRoot = path.join(root, 'packages/editor');
const sourceRoot = path.join(packageRoot, 'src');
const configPath = path.join(root, 'tsconfig.editor-package.json');
const loaded = ts.readConfigFile(configPath, ts.sys.readFile);
if (loaded.error)
  throw new Error(
    ts.flattenDiagnosticMessageText(loaded.error.messageText, '\n'),
  );
const config = ts.parseJsonConfigFileContent(loaded.config, ts.sys, root);
const program = ts.createProgram(config.fileNames, config.options);
const emit = program.emit();
const diagnostics = [...ts.getPreEmitDiagnostics(program), ...emit.diagnostics];
if (diagnostics.length) {
  process.stderr.write(
    ts.formatDiagnosticsWithColorAndContext(diagnostics, {
      getCanonicalFileName: (name) => name,
      getCurrentDirectory: () => root,
      getNewLine: () => '\n',
    }),
  );
  process.exitCode = 1;
} else {
  const outputRoot = path.join(packageRoot, 'dist/types');
  for (const source of program.getSourceFiles()) {
    const relative = path.relative(sourceRoot, source.fileName);
    if (
      !source.isDeclarationFile ||
      relative.startsWith('..') ||
      path.isAbsolute(relative)
    )
      continue;
    const target = path.join(outputRoot, relative);
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, source.text);
  }
  await writeFile(
    path.join(packageRoot, 'dist/LICENSE'),
    await readFile(path.join(root, 'LICENSE')),
  );
  const aliasPrefixes = Object.keys(config.options.paths ?? {})
    .filter((name) => name.endsWith('/*'))
    .map((name) => name.slice(0, -1));
  const sourceFiles = new Map(
    program
      .getSourceFiles()
      .map((source) => [
        source.fileName.replace(/\.(?:d\.)?[cm]?[jt]sx?$/, ''),
        source.fileName,
      ]),
  );
  const visit = async (directory: string): Promise<void> => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) await visit(file);
      else if (entry.name.endsWith('.d.ts')) {
        const original = await readFile(file, 'utf8');
        const declaration = ts.createSourceFile(
          file,
          original,
          ts.ScriptTarget.Latest,
          true,
        );
        const moduleReferences: ts.StringLiteralLike[] = [];
        const collectReferences = (node: ts.Node): void => {
          if (
            (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
            node.moduleSpecifier &&
            ts.isStringLiteralLike(node.moduleSpecifier)
          ) {
            moduleReferences.push(node.moduleSpecifier);
          } else if (
            ts.isImportTypeNode(node) &&
            ts.isLiteralTypeNode(node.argument) &&
            ts.isStringLiteralLike(node.argument.literal)
          ) {
            moduleReferences.push(node.argument.literal);
          } else if (
            ts.isExternalModuleReference(node) &&
            node.expression &&
            ts.isStringLiteralLike(node.expression)
          ) {
            moduleReferences.push(node.expression);
          }
          ts.forEachChild(node, collectReferences);
        };
        collectReferences(declaration);
        let source = original;
        // 뒤에서 치환해 앞선 문자열의 원본 위치 유지. ambient module 선언은 의존 import가 아님
        for (const reference of moduleReferences.sort(
          (a, b) => b.getStart(declaration) - a.getStart(declaration),
        )) {
          const specifier = reference.text;
          if (!aliasPrefixes.some((prefix) => specifier.startsWith(prefix)))
            continue;
          const sourceKey = path
            .join(sourceRoot, path.relative(outputRoot, file))
            .replace(/\.d\.ts$/, '');
          const containingFile = sourceFiles.get(sourceKey);
          if (!containingFile)
            throw new Error(`Declaration source not found: ${file}`);
          const resolved = ts.resolveModuleName(
            specifier,
            containingFile,
            config.options,
            ts.sys,
          ).resolvedModule?.resolvedFileName;
          if (!resolved)
            throw new Error(
              `Declaration alias cannot resolve: ${specifier} in ${containingFile}`,
            );
          const relativeSource = path.relative(sourceRoot, resolved);
          if (
            relativeSource === '..' ||
            relativeSource.startsWith(`..${path.sep}`) ||
            path.isAbsolute(relativeSource)
          ) {
            throw new Error(
              `Editor declaration depends on source outside its package: ${specifier} -> ${resolved}`,
            );
          }
          const target = path.join(
            outputRoot,
            relativeSource.replace(/\.(?:d\.)?[cm]?[jt]sx?$/, ''),
          );
          let relative = path
            .relative(path.dirname(file), target)
            .replaceAll(path.sep, '/');
          if (!relative.startsWith('.')) relative = `./${relative}`;
          source =
            source.slice(0, reference.getStart(declaration) + 1) +
            relative +
            source.slice(reference.getEnd() - 1);
        }
        await writeFile(file, source);
      }
    }
  };
  await visit(outputRoot);
}
