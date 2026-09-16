// Validate every downloaded manifest before publishing any of the tarballs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

function verifyReleaseManifests(directory, version) {
	assert.ok(directory && version, 'expected a manifest directory and release version');
	const targets = ['linux-arm64', 'linux-x64', 'darwin-arm64', 'darwin-x64'];
	function read(name) {
		const manifest = JSON.parse(fs.readFileSync(path.join(directory, `${name}.json`), 'utf8'));
		assert.equal(manifest.version, version);
		assert.equal(manifest.scripts, undefined, 'release packages must not run lifecycle scripts');
		assert.equal(manifest.devDependencies, undefined);
		assert.ok(!JSON.stringify(manifest).includes('workspace:'), 'unresolved workspace dependency');
		return manifest;
	}
	const launcher = read('ccusage');
	assert.equal(launcher.name, 'ccusage');
	assert.deepEqual(launcher.bin, { ccusage: './src/cli.js' });
	assert.deepEqual(
		[...launcher.os].sort((a, b) => a.localeCompare(b)),
		['darwin', 'linux'],
	);
	assert.deepEqual(
		[...launcher.cpu].sort((a, b) => a.localeCompare(b)),
		['arm64', 'x64'],
	);
	assert.deepEqual(
		launcher.optionalDependencies,
		Object.fromEntries(targets.map((target) => [`@ccusage/ccusage-${target}`, version])),
	);
	const artifacts = targets.map((target) => {
		const manifest = read(`ccusage-ccusage-${target}`);
		const [os, cpu] = target.split('-');
		assert.equal(manifest.name, `@ccusage/ccusage-${target}`);
		assert.deepEqual(manifest.os, [os]);
		assert.deepEqual(manifest.cpu, [cpu]);
		assert.deepEqual(manifest.files, ['bin/ccusage']);
		return { name: manifest.name, filename: `ccusage-ccusage-${target}-${version}.tgz` };
	});
	return [...artifacts, { name: launcher.name, filename: `ccusage-${version}.tgz` }];
}

module.exports = { verifyReleaseManifests };
if (require.main === module) verifyReleaseManifests(...process.argv.slice(2));
