const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { publishRelease } = require('./publish-release.cjs');
const { verifyReleaseManifests } = require('./verify-release-manifests.cjs');

const version = '1.2.3';
const targets = ['linux-arm64', 'linux-x64', 'darwin-arm64', 'darwin-x64'];
const registry = 'https://registry.npmjs.org';
const missing = {
	status: 1,
	stdout: JSON.stringify({ error: { code: 'E404' } }),
	stderr: 'npm error code E404',
};
const success = (integrity) => ({ status: 0, stdout: JSON.stringify(integrity) });

function fixture(t) {
	const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ccusage-publish-'));
	t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
	const manifests = targets.map((target) => {
		const [os, cpu] = target.split('-');
		return {
			name: `@ccusage/ccusage-${target}`,
			version,
			os: [os],
			cpu: [cpu],
			files: ['bin/ccusage'],
		};
	});
	manifests.push({
		name: 'ccusage',
		version,
		bin: { ccusage: './src/cli.js' },
		os: ['darwin', 'linux'],
		cpu: ['arm64', 'x64'],
		optionalDependencies: Object.fromEntries(
			targets.map((target) => [`@ccusage/ccusage-${target}`, version]),
		),
	});
	const artifacts = manifests.map((manifest) => {
		const stem = manifest.name.replace('@', '').replace('/', '-');
		const manifestPath = path.join(directory, `${stem}.json`);
		const tarball = path.join(directory, `${stem}-${version}.tgz`);
		// The shell extracts real tarballs; this helper only needs their exact bytes.
		const bytes = Buffer.from(`finished tarball for ${manifest.name}\0`);
		fs.writeFileSync(manifestPath, JSON.stringify(manifest));
		fs.writeFileSync(tarball, bytes);
		return {
			manifest,
			manifestPath,
			tarball,
			spec: `${manifest.name}@${version}`,
			integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}`,
		};
	});
	const calls = [];
	const logs = [];
	function publish(respond) {
		publishRelease(directory, directory, version, {
			run(command, args, options) {
				assert.equal(command, 'npm');
				calls.push(args);
				const artifact = artifacts.find(
					(item) => args[1] === (args[0] === 'view' ? item.spec : item.tarball),
				);
				assert.ok(artifact, 'only an exact release version or expected tarball may be used');
				if (args[0] === 'view') {
					assert.deepEqual(args, [
						'view',
						artifact.spec,
						'dist.integrity',
						'--json',
						'--prefer-online',
						'--registry',
						registry,
					]);
					assert.deepEqual(options, { encoding: 'utf8' });
				} else {
					assert.deepEqual(args, [
						'publish',
						artifact.tarball,
						'--ignore-scripts',
						'--provenance',
						'--access',
						'public',
						'--registry',
						registry,
					]);
					assert.deepEqual(options, { stdio: 'inherit' });
				}
				return respond(args[0], artifact);
			},
			log: (message) => logs.push(message),
		});
	}
	return { directory, artifacts, calls, logs, publish };
}

test('an entirely published release skips all five matching SHA512 integrities', (t) => {
	const f = fixture(t);
	f.publish((command, artifact) => {
		assert.equal(command, 'view');
		return success(artifact.integrity);
	});
	assert.deepEqual(
		f.calls.map((args) => args[1]),
		f.artifacts.map((item) => item.spec),
	);
	assert.equal(f.logs.length, 5);
});

test('only structured E404 publishes, with all native packages before the launcher', (t) => {
	const f = fixture(t);
	f.publish((command) => (command === 'view' ? missing : { status: 0 }));
	assert.deepEqual(
		f.calls.map((args) => args.slice(0, 2)),
		f.artifacts.flatMap((item) => [
			['view', item.spec],
			['publish', item.tarball],
		]),
	);
	assert.equal(f.logs.length, 0);
});

test('retry after partial success publishes only the remaining identical release artifacts', (t) => {
	const f = fixture(t);
	const published = new Map();
	let fail = true;
	function respond(command, artifact) {
		if (command === 'view')
			return published.has(artifact.spec) ? success(published.get(artifact.spec)) : missing;
		if (fail && artifact === f.artifacts[2]) return { status: 1 };
		published.set(artifact.spec, artifact.integrity);
		return { status: 0 };
	}
	assert.throws(() => f.publish(respond), /npm publish .* failed/);
	assert.equal(published.size, 2);
	assert.ok(
		!f.calls.some((args) => args[1] === f.artifacts[4].spec),
		'stop before the launcher after failure',
	);
	f.calls.length = 0;
	fail = false;
	f.publish(respond);
	assert.deepEqual(
		f.calls.filter((args) => args[0] === 'publish').map((args) => args[1]),
		f.artifacts.slice(2).map((item) => item.tarball),
	);
	assert.equal(published.size, 5);
	assert.equal(f.logs.length, 2);
});

test('a differing published integrity aborts rather than skipping or overwriting', (t) => {
	const f = fixture(t);
	assert.throws(() => f.publish(() => success('sha512-different')), /Integrity mismatch/);
	assert.equal(f.calls.length, 1);
});

for (const [label, result] of [
	...['E401', 'E403', 'ETIMEDOUT', 'ENOTFOUND', 'E500'].map((code) => [
		code,
		{ status: 1, stdout: JSON.stringify({ error: { code } }) },
	]),
	['unstructured E404', { status: 1, stdout: 'E404 not found' }],
	['stderr alone', { status: 1, stdout: '', stderr: 'npm error code E404' }],
	['malformed error JSON', { status: 1, stdout: '{"error":{"code":"E404"}' }],
	['wrong error shape', { status: 1, stdout: '{"code":"E404"}' }],
	['signal with E404 output', { ...missing, status: null, signal: 'SIGTERM' }],
	['spawn error', { ...missing, error: new Error('could not execute npm') }],
	['successful error payload', { ...missing, status: 0 }],
	['missing integrity', { status: 0, stdout: '' }],
	['empty integrity', success('')],
	['null integrity', success(null)],
	['object instead of integrity', success({ integrity: 'sha512-value' })],
	['multiple integrities', success(['sha512-value'])],
]) {
	if (typeof label !== 'string') throw new TypeError('Test case labels must be strings');
	test(`${label} fails closed without publishing`, (t) => {
		const f = fixture(t);
		assert.throws(() => f.publish(() => result));
		assert.equal(f.calls.length, 1);
		assert.equal(f.calls[0][0], 'view');
	});
}

test('a publish subprocess error also stops the release', (t) => {
	const f = fixture(t);
	assert.throws(
		() =>
			f.publish((command) =>
				command === 'view' ? missing : { error: new Error('spawn failure') },
			),
		/spawn failure/,
	);
	assert.equal(f.calls.length, 2);
});

for (const [label, index, mutate] of [
	[
		'launcher version',
		4,
		(m) => {
			m.version = '1.2.2';
		},
	],
	[
		'launcher name',
		4,
		(m) => {
			m.name = 'other';
		},
	],
	[
		'launcher bin',
		4,
		(m) => {
			m.bin.ccusage = './other.js';
		},
	],
	[
		'Windows launcher support',
		4,
		(m) => {
			m.os.push('win32');
		},
	],
	[
		'launcher CPU',
		4,
		(m) => {
			m.cpu.push('ia32');
		},
	],
	[
		'missing optional package',
		4,
		(m) => {
			delete m.optionalDependencies['@ccusage/ccusage-linux-x64'];
		},
	],
	[
		'mismatched optional version',
		4,
		(m) => {
			m.optionalDependencies['@ccusage/ccusage-linux-x64'] = '1.2.2';
		},
	],
	[
		'extra Windows optional package',
		4,
		(m) => {
			m.optionalDependencies['@ccusage/ccusage-win32-x64'] = version;
		},
	],
	[
		'last native version',
		3,
		(m) => {
			m.version = '1.2.2';
		},
	],
	[
		'last native name',
		3,
		(m) => {
			m.name = '@ccusage/other';
		},
	],
	[
		'last native OS',
		3,
		(m) => {
			m.os = ['linux'];
		},
	],
	[
		'last native CPU',
		3,
		(m) => {
			m.cpu = ['arm64'];
		},
	],
	[
		'last native files',
		3,
		(m) => {
			m.files.push('extra');
		},
	],
	[
		'last native scripts',
		3,
		(m) => {
			m.scripts = { install: 'bad' };
		},
	],
	[
		'last native dev dependencies',
		3,
		(m) => {
			m.devDependencies = {};
		},
	],
	[
		'last native workspace dependency',
		3,
		(m) => {
			m.dependencies = { other: 'workspace:*' };
		},
	],
]) {
	if (typeof label !== 'string') throw new TypeError('Test case labels must be strings');
	test(`retry validates ${label} before any registry request`, (t) => {
		const f = fixture(t);
		const artifact = f.artifacts[index];
		mutate(artifact.manifest);
		fs.writeFileSync(artifact.manifestPath, JSON.stringify(artifact.manifest));
		assert.throws(() => f.publish((command, item) => success(item.integrity)));
		assert.equal(f.calls.length, 0);
	});
}

for (const field of ['manifestPath', 'tarball']) {
	test(`missing final ${field} aborts before any registry request, including on retry`, (t) => {
		const f = fixture(t);
		fs.unlinkSync(f.artifacts[4][field]);
		assert.throws(() => f.publish((command, item) => success(item.integrity)), /ENOENT/);
		assert.equal(f.calls.length, 0);
	});
}

test('standalone offline manifest validator remains usable and returns exactly five artifacts', (t) => {
	const f = fixture(t);
	assert.deepEqual(
		verifyReleaseManifests(f.directory, version),
		f.artifacts.map((item) => ({
			name: item.manifest.name,
			filename: path.basename(item.tarball),
		})),
	);
	execFileSync(process.execPath, [
		path.join(__dirname, 'verify-release-manifests.cjs'),
		f.directory,
		version,
	]);
	assert.throws(() =>
		execFileSync(
			process.execPath,
			[path.join(__dirname, 'verify-release-manifests.cjs'), f.directory, '1.2.2'],
			{ stdio: 'pipe' },
		),
	);
});
