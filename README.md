# rconfd

`rconfd` is a lightweight utility for containers, written in async rust, to generate
config files from [jsonnet templates](https://jsonnet.org/), and keep them in sync
whith secrets fetched from a [vault server](https://www.vaultproject.io/) with [kubernetes
authentification](https://www.vaultproject.io/docs/auth/kubernetes). It can use the simple and yet effective
[startup notification](https://skarnet.org/software/s6/notifywhenup.html) mechanism of the [s6 supervision
suite](https://skarnet.org/software/s6/) to signal other services that their configuration files have been generated
and it can launch arbitrary command when configuration change.

# Yet another configuration template manager ?

There is a lot of alternatives for generating configuration files under kubernetes with various template engines
and secrets backends, but because it can run potentially in hundreds of containers in the same host, I wanted the
lightest, fastest with minimal attack surface, while using the lesser resources at the same time, even at the cost
of flexibility (few backends, one template engine). Rust beat C/C++ and all other languages in all those aspects
by a comfortable margin while giving you correctness and easy maintenance with no special efforts.

Like the [S6 overlay authors](https://github.com/just-containers/s6-overlay#the-docker-way), I never believed in
the rigid general approach of one executable per container, which forces you to decouple your software stack under
kubernetes into init containers, inject containers, side car containers, with liveness and readiness tests and blind
kill and restart if conditions are not not met. With several service in a container, the orchestration is simple and
smarter, it starts faster, and scale better without putting unecessary pressure on your orchestration supervisor
or container runtime.

`rconfd` is a rewrite of the C++ `cconfd` utility using the
[blazing fast](https://github.com/CertainLach/jrsonnet#Benchmarks) [jrsonnet
interpreter](https://github.com/CertainLach/jrsonnet). `cconfd` while working, was a failed attempt using stdc++17
and the [google/fruit](https://github.com/google/fruit) dependency injection library. It was way too hard to maintain
I never managed to add the missing features or fix some bugs.

Once you know rust, you are just forced to recognize that despite its iterations, c++ is just getting older,
weirder and dustier. `rconfd` is way better than `cconfd` in every aspects: feature complete, faster, smarter,
lighter, maintainable, thread and memory safe.

# jsonnet ?

Configuration files are structured by nature. Using a text templating system for generating them expose you to
malformations (you forgot to close a `(` or a `{` or didn't indent correcly inside a loop, ...), injection attacks,
and escaping hell. With jsonnet it's impossible to generate malformed files (unless you use string manifestation
which defeat the purpose of using jsonnet in the first place).

Jsonnet permit using complex operations for merging, adding, overriding and allowing you to easily and securly
specialize your configuration files


## Usage

```
rconfd 0.1.0

Usage: rconfd -d <dir> [-u <url>] [-c <cacert>] [-t <token>] [-V] [-r <ready-fd>] [-D]

Generate config files from jsonnet templates and keep them in sync with secrets fetched from a vault server with kubernetes authentification.

Options:
  -d, --dir         directory containing the rconfd config files
  -u, --url         the vault url (https://localhost:8200)
  -c, --cacert      path of the service account
                    certificate	(/var/run/secrets/kubernetes.io/serviceaccount/ca.crt)
  -t, --token       path of the kubernetes token
                    (/var/run/secrets/kubernetes.io/serviceaccount/token)
  -V, --verbose     verbose mode
  -r, --ready-fd    s6 readiness file descriptor
  -D, --daemon      daemon mode (no detach)
  --help            display usage information
```

`rconfd` takes its instructions from one or several json files laying inside a directory (`-d` argument).

Each configuration file must follow the following structure. For instance let's create a `test.json` file

```json
{
	"tmpl": "test.jsonnet",
	"paths": ["/etc/rconfd"],
	"conf": {
		"dir": "/etc/test",
		"mode": "0644",
		"user": "test-user",
		"role": "test-role",
		"secrets": {
			"vault:kv/data/test/mysecret": "mysecret",
			"env:NAMESPACE": "namespace",
			"file:file.json": "file"
		},
		"cmd": "echo reload"
	}
}
```

The template `tmpl` is a multi output jsonnet template. The root keys of the template represents path of the files
to be generated, and the value of the key represent the template for the file. `dir` is used if a key is a relative
path, `user` and `mode` set the owner and file permissions on successful manifestation.

`secrets` is a map of json value inserted in an `extVar` variable before interpreting the jsonnet template. The
key is a path from a backend and value is the name of top level key inside the `secrets` jsonnet extVar.

There are currently 3 backends:
- `vault`: fetch a secret from the vault server (`-u` argument) using `role`
- `env`: fetch the environment variable and parse it as a json
- `file`: fetch the content of the file and parse it as a json value

The secrets are collected among all config files (to fetch each secret only once) and the `cmd` is executed if
any of the config file change after manifestation.

`paths` is a list of additional jsonnet library paths if your template used anything other than stdlib.

# Example template

You should correctly
- setup a vault server,
- activate one or several secret backends,
- create a role allowed to access the secrets inside the backends,
- create the secret

Using the `test.json` file above, we could write the following `test.jsonnet` template to create a `config.json`
inside the `/etc/test` directory.

```jsonnet
{
	// we define shortcuts for easy access to the secret extVar
	// the :: is to tell jsonnet to not consider the key as a file to generate
	secrets:: std.extVar("secrets"),
	// we have data and metadata with vault kv v2, so go directly to the point
	mysecret:: self.secrets['mysecret']['data']['data'],
	namespace:: self.secrets['namespace'],
	file:: self.secrets['file'],

	// just dump all secrets using json manifestation
	'config.json': std.manifestJsonEx({
		mysecret: $.mysecret,
		namespace: $.namespace,
		file: $.file
	}, '  ')
}

```
