# rconfd

`rconfd` is a lightweight utility for containers, written in async rust, to generate
config files from [jsonnet templates](https://jsonnet.org/), and keep them in sync
whith secrets fetched from a [vault server](https://www.vaultproject.io/) with [kubernetes
authentication](https://www.vaultproject.io/docs/auth/kubernetes). It can use the simple and yet effective
[startup notification](https://skarnet.org/software/s6/notifywhenup.html) mechanism of the [s6 supervision
suite](https://skarnet.org/software/s6/) to signal other services that their configuration files have been generated
and it can launch arbitrary command when configuration change.

# Yet another configuration template manager ?

There is a lot of alternatives for generating configuration files at runtime under kubernetes with
various template engines and secrets back-ends ([cconfd](https://github.com/kelseyhightower/confd),
[consul-template](https://github.com/hashicorp/consul-template)... but because it
can run a lot of containers in the same host, I wanted the lightest and fastest implementation as possible with a
minimal surface attack, even at the cost of flexibility (few back-ends, one template engine). Rust beats C/C++ and
all other languages in all those aspects by a comfortable margin while giving you correctness and easy maintenance
with no special efforts.

Like the [S6 overlay authors](https://github.com/just-containers/s6-overlay#the-docker-way), I never believed
in the rigid general approach of one executable per container, which forces you to decouple your software
stack under kubernetes into init containers, inject containers, side car containers, with liveliness
and readiness tests and blind kill and restart on timeout if conditions are not not met (like [vault
injector](https://learn.hashicorp.com/tutorials/vault/kubernetes-sidecar?in=vault/kubernetes)). With several
service in a container, the orchestration is simple and smarter, it starts faster, and scale better without putting
unnecessary pressure on your orchestration supervisor or container runtime.

`rconfd` is a rewrite of the C++ [cconfd](https://github.com/eburghar/cconfd) utility
using the [blazing fast](https://github.com/CertainLach/jrsonnet#Benchmarks) [jrsonnet
interpreter](https://github.com/CertainLach/jrsonnet). `cconfd`, while working, was a failed attempt using stdc++17
and the [google/fruit](https://github.com/google/fruit) dependency injection library. It was way too hard to
understand and maintain. I never managed to allocate resources to add the missing features or fix some obvious
bugs. In contrast I ported all `cconfd` features for `rconfd` in just 2 days.

Once you know rust, you are just forced to recognize that despite its iterations, c++ is just getting older,
weirder and dustier. `rconfd` is way better than `cconfd` in every aspects: feature complete, faster, smarter,
lighter, maintainable, thread and memory safe.

# jsonnet ?

Configuration files are structured by nature. Using a text templating system for generating them expose you to
malformations (you forgot to close a `(` or a `{` or didn't indent correctly inside a loop, ...), injection attacks,
and escaping hell. With jsonnet it's impossible to generate malformed files (unless you use string manifestation
which defeat the purpose of using jsonnet in the first place).

Jsonnet permits using complex operations for merging, adding, overriding and allowing you to easily and securely
specialize your configuration files. By using mounts or environment variables in your kubernetes manifests, along
with the `file` and `env` back-ends, you can easily compose your configuration files at startup.

## Usage

```
rconfd 0.6.0

Usage: rconfd -d <dir> [-u <url>] [-j <jpath>] [-c <cacert>] [-t <token>] [-V] [-r <ready-fd>] [-D]

Generate config files from jsonnet templates and keep them in sync with secrets fetched from a vault server with kubernetes authentication.

Options:
  -d, --dir         directory containing the rconfd config files
  -u, --url         the vault url (https://localhost:8200)
  -j, --jpath       , separated list of additional path for jsonnet libraries
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

Each configuration declares one or several jsonnet template files which in turn generate one or several configuration
files.

Here is a simple `test.json` file declaring only one template, and using all 3 different secrets backends. We use
4 different secrets engines with the `vault` backend ([kv-v2](https://www.vaultproject.io/docs/secrets/kv/kv-v2),
[pki](https://www.vaultproject.io/docs/secrets/pki), [databases](https://www.vaultproject.io/docs/secrets/databases),
[transit](https://www.vaultproject.io/docs/secrets/transit)) which require using 2 differents http methods (`GET`
by default, and `POST`).

```json
{
	"test.jsonnet": {
		"dir": "/etc/test",
		"mode": "0644",
		"user": "test-user",
		"secrets": {
			"vault:${NAMESPACE}-role:kv/data/test/mysecret": "mysecret",
			"vault:${NAMESPACE}-role:database/creds/mydb": "mydb",
			"vault:${NAMESPACE}-role,POST,common_name=example.com:pki/issue/example.com": "cert",
			"vault:${NAMESPACE}-role,POST,input=password:transit/hmac/mysecret": "mysecret2",
			"env:str:NAMESPACE": "namespace",
			"file:js:file.json": "file"
		},
		"cmd": "echo reload"
	}
}
```

The template `test.jsonnet` is a multi file output jsonnet template which means that the root keys represent the paths
of the files to be generated, while the values represent the templates. `dir` is used if a key is a relative path,
`user` and `mode` set the owner and file permissions on successful manifestation.

`secrets` maps secret path to variable name, and are accessible inside jsonnet templates through a
`secrets` [extVar](https://jsonnet.org/ref/stdlib.html) variable. The path has the following syntax:
`backend:arg1,arg2,k1=v1,k2=v2:path`, and can contain environment variables expressions `${NAME}`, in which case
it is the resulting string, after substitutions, that should conform to the aforementioned syntax.

There are currently 3 supported back-ends:
- `vault`: fetch a secret from the vault server using `arg1` as a `role` name for authentication, and `arg2` as the
   optional http method (`GET` by default). keywords arguments are sent in the body of the request.
- `env`: fetch the environment variable and parse it as a json if `args` == `js` or keep it as a string if `str`
- `file`: fetch the content of the file and parse it as a json value if `args` == `js` or keep it as a string if `str`

The secrets are collected among all templates and all config files (to fetch each secret only once) and the `cmd`
is executed if any of the config file change after manifestation.

# Example

You should correctly
- [setup a vault server](https://learn.hashicorp.com/tutorials/vault/kubernetes-raft-deployment-guide?in=vault/kubernetes)
- [activate one or several secret engines](https://www.vaultproject.io/docs/secrets),
- [activate kubernetes auth method](https://www.vaultproject.io/docs/auth/kubernetes),
- [create policies](https://www.vaultproject.io/docs/concepts/policies) allowing your roles to access the secrets
  inside the back-ends
- create some secrets

Using the `test.json` file above, we could write the following `test.jsonnet` template to create a `config.json`
inside the `/etc/test` directory.

```jsonnet
local secrets = std.extVar("secrets");
{
	// we define shortcuts for easy access to the secret extVar
	// the :: is to tell jsonnet to not consider the key as a file to generate
	// kv2 secret backend contains data and metadata so go directly to the point
	mysecret:: secrets['mysecret'],
	mysecret2:: secrets['mysecret2'],
	namespace:: secrets['namespace'],
	file:: secrets['file'],
	cert:: secrets['cert'],

	// just dump all secrets using json manifestation
	'config.json': std.manifestJsonEx({
		mysecret: $.mysecret,
		mysecret2: std.split($.mysecret2.hmac, ':')[2],
		namespace: $.namespace,
		file: $.file
	}, '  ')

	// save certificate and key in separate files
	'cert.crt': $.cert['certificate'],
	'cert.key': $.cert['private_key'],

	// conditional file manifestation
	[if secrets['file']['test'] == 'true' then 'key']: 'val'
}
```
