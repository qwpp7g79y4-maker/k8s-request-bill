# Security

## Reporting a vulnerability

Send a private report to pep_ai@icloud.com. Please do not open a public issue
for a vulnerability.

Include the version (`k8s-request-bill --version`), what you did, what you
expected and what happened. A small input file that shows the problem helps.

## Scope

The tool reads files or stdin and prints a report. It makes no network calls,
needs no credentials and writes no files. Relevant reports are, for example,
crashes or wrong numbers on crafted input.

## Supported versions

Only the latest release (0.1.0 at the time of writing) is supported.
