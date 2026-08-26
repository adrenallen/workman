#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputPath = resolve(process.argv[2] ?? join(repoRoot, 'THIRD_PARTY_NOTICES.generated.md'));
const noticeFiles = /^(?:licen[cs]e|copying|notice|unlicense)(?:[._-].*)?$/i;
const packages = new Map();

function licenseTexts(packageDir) {
  const texts = [];
  for (const entry of readdirSync(packageDir, { withFileTypes: true })) {
    if (!entry.isFile() || !noticeFiles.test(entry.name)) continue;
    const file = join(packageDir, entry.name);
    if (statSync(file).size > 1024 * 1024) continue;
    const text = readFileSync(file, 'utf8').replace(/\r\n/g, '\n').trim();
    if (text && !text.includes('\0')) texts.push({ name: entry.name, text });
  }
  return texts;
}

function addPackage(ecosystem, name, version, license, packageDir) {
  const key = `${ecosystem}:${name}@${version}`;
  if (packages.has(key)) return;
  packages.set(key, {
    key,
    label: `${name} ${version}`,
    ecosystem,
    license: typeof license === 'string' && license.trim() ? license.trim() : 'not declared',
    texts: licenseTexts(packageDir),
  });
}

const cargoMetadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--locked', '--format-version', '1'], {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  }),
);
const workspaceMembers = new Set(cargoMetadata.workspace_members);
for (const pkg of cargoMetadata.packages) {
  if (!workspaceMembers.has(pkg.id)) {
    addPackage('Cargo', pkg.name, pkg.version, pkg.license, dirname(pkg.manifest_path));
  }
}

const seenNodeModules = new Set();
function scanNodeModules(nodeModulesDir) {
  if (!existsSync(nodeModulesDir)) return;
  const canonical = realpathSync(nodeModulesDir);
  if (seenNodeModules.has(canonical)) return;
  seenNodeModules.add(canonical);

  for (const entry of readdirSync(nodeModulesDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith('.')) continue;
    const firstLevel = join(nodeModulesDir, entry.name);
    const packageDirs = entry.name.startsWith('@')
      ? readdirSync(firstLevel, { withFileTypes: true })
          .filter((child) => child.isDirectory())
          .map((child) => join(firstLevel, child.name))
      : [firstLevel];

    for (const packageDir of packageDirs) {
      const manifest = join(packageDir, 'package.json');
      if (!existsSync(manifest)) continue;
      const pkg = JSON.parse(readFileSync(manifest, 'utf8'));
      addPackage('npm', pkg.name ?? packageDir.split('/').at(-1), pkg.version ?? 'unknown', pkg.license, packageDir);
      scanNodeModules(join(packageDir, 'node_modules'));
    }
  }
}

scanNodeModules(join(repoRoot, 'apps', 'desktop', 'node_modules'));

const groupedTexts = new Map();
const missingTexts = [];
for (const pkg of [...packages.values()].sort((a, b) => a.key.localeCompare(b.key))) {
  if (pkg.texts.length === 0) {
    missingTexts.push(pkg);
    continue;
  }
  for (const item of pkg.texts) {
    const hash = createHash('sha256').update(item.text).digest('hex');
    const group = groupedTexts.get(hash) ?? { packages: [], text: item.text };
    group.packages.push(`${pkg.ecosystem}: ${pkg.label} (${pkg.license}; ${item.name})`);
    groupedTexts.set(hash, group);
  }
}

const output = [readFileSync(join(repoRoot, 'THIRD_PARTY_NOTICES.md'), 'utf8').trim()];
output.push('\n## Locked dependency license files');
output.push(
  '\nThe following texts were collected from the exact Cargo and desktop npm packages installed from the lockfiles.',
);
for (const group of [...groupedTexts.values()].sort((a, b) => a.packages[0].localeCompare(b.packages[0]))) {
  output.push(`\n### ${group.packages.join('; ')}\n\n\`\`\`text\n${group.text}\n\`\`\``);
}

if (missingTexts.length > 0) {
  output.push('\n## Dependencies without a packaged license file');
  output.push('\nThese packages declared the following license expression but did not ship a top-level license file:');
  for (const pkg of missingTexts) {
    output.push(`\n- ${pkg.ecosystem}: ${pkg.label} — ${pkg.license}`);
  }
}

output.push('');
writeFileSync(outputPath, output.join('\n'), 'utf8');
console.log(`Wrote ${outputPath} (${packages.size} dependencies, ${groupedTexts.size} unique license texts)`);
