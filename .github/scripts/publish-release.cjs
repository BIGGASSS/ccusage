const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { verifyReleaseManifests } = require('./verify-release-manifests.cjs');

const registry = 'https://registry.npmjs.org';

function publishedIntegrity(spec, run) {
	const result = run(
		'npm',
		['view', spec, 'dist.integrity', '--json', '--prefer-online', '--registry', registry],
		{
			encoding: 'utf8',
		},
	);
	if (result.error) throw result.error;
	let data;
	try {
		data = JSON.parse(result.stdout);
	} catch {
		// Invalid/missing JSON is never evidence that a version is unpublished.
	}
	if (result.status !== 0) {
		// Only npm's structured E404 means missing. Auth, network, signal and
		// malformed-output failures must stop the release, not trigger a publish.
		if (result.status > 0 && !result.signal && data?.error?.code === 'E404') return null;
		throw new Error(
			`npm view ${spec} failed: ${result.stderr || result.stdout || result.signal || result.status}`,
		);
	}
	if (typeof data !== 'string' || !data)
		throw new Error(`npm view ${spec} returned no dist.integrity`);
	return data;
}

function publishRelease(
	directory,
	manifestDirectory,
	version,
	{ manual = false, run = spawnSync, log = console.log } = {},
) {
	// Always validate the entire release, even on retries where some (or all)
	// versions already exist. Read/hash every local tarball before any network IO.
	const artifacts = verifyReleaseManifests(manifestDirectory, version).map(({ name, filename }) => {
		const tarball = path.resolve(directory, filename);
		const integrity = `sha512-${createHash('sha512').update(fs.readFileSync(tarball)).digest('base64')}`;
		return { name, tarball, integrity };
	});
	for (const { name, tarball, integrity } of artifacts) {
		const spec = `${name}@${version}`;
		const published = publishedIntegrity(spec, run);
		if (published !== null) {
			if (published !== integrity)
				throw new Error(`Integrity mismatch for ${spec}; refusing to republish`);
			log(`Skipping ${spec}: identical tarball already published`);
			continue;
		}
		const result = run(
			'npm',
			[
				'publish',
				tarball,
				'--ignore-scripts',
				manual ? '--provenance=false' : '--provenance',
				'--access',
				'public',
				'--registry',
				registry,
			],
			{ stdio: 'inherit' },
		);
		if (result.error) throw result.error;
		if (result.status !== 0)
			throw new Error(`npm publish ${spec} failed (${result.signal || result.status})`);
	}
}

module.exports = { publishRelease };
if (require.main === module) {
	try {
		const args = process.argv.slice(2);
		const manual = args[0] === '--manual';
		if (manual) args.shift();
		if (args.length !== 3 || args.some((arg) => !arg || arg.startsWith('-')))
			throw new Error(
				'usage: node publish-release.cjs [--manual] <tarball-directory> <manifest-directory> <release-version>',
			);
		publishRelease(...args, { manual });
	} catch (error) {
		console.error(error.message);
		process.exitCode = 1;
	}
}
