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
various template engines and secrets back-ends ([confd](https://github.com/kelseyhightower/confd),
[consul-template](https://github.com/hashicorp/consul-template)...) but because it can run a lot of containers
in the same host, I wanted the lightest and fastest implementation as possible with a minimal surface attack,
even at the cost of flexibility (few back-ends, one template engine). Rust match C/C++ speed while giving you
safeness, correctness and easy maintenance with no special efforts.

Like the [S6 overlay authors](https://github.com/just-containers/s6-overlay#the-docker-way), I never believed
in the rigid general approach of one executable per container, which forces you to decouple your software stack
under kubernetes into init containers, inject containers, side car containers, with liveliness and readiness
tests and blind kill and restart on timeout if conditions are not not met (which is the approach of [vault
injector](https://learn.hashicorp.com/tutorials/vault/kubernetes-sidecar?in=vault/kubernetes)). With several service
in a container, the orchestration is simple and smarter, it starts faster, and scale without putting unnecessary
pressure on your orchestration supervisor or container runtime.

`rconfd` is a rewrite of the C++ [cconfd](https://github.com/eburghar/cconfd) utility
using the [blazing fast](https://github.com/CertainLach/jrsonnet#Benchmarks) [jrsonnet
interpreter](https://github.com/CertainLach/jrsonnet). `cconfd`, while working, was a failed attempt using
stdc++17 and the [google/fruit](https://github.com/google/fruit) dependency injection library. It was way too
hard to understand and maintain. I never managed to allocate resources to add the missing features or fix some
obvious bugs. In contrast I ported all `cconfd` features in `rconfd` in just 2 days, and now as I consider it
feature complete, I know it is also faster, smarter, lighter, maintainable, thread and memory safe.

# jsonnet ?

Configuration files are structured by nature. Using a text templating system ([mustache](https://mustache.github.io/)
like) for generating them expose you to malformations (you forgot to close a `(` or a `{`, bad indent in a loop,
...), injection attacks, and escaping hell. With jsonnet it's impossible to generate malformed files, unless you
use string templates, which defeat the purpose of using jsonnet (objects) in the first place.

Jsonnet permits using complex operations for merging, adding, overriding and allowing you to easily and securely
specialize your configuration files. By using mounts or environment variables in your kubernetes manifests, along
with the `file` and `env` back-ends, you can easily compose your configuration files at startup in a flexible way.

# Usage

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

Here is a simple `test.json` file declaring only one template, and using 3 different
secrets backends (`vault`, `env` and `file`). We also use 4 different secrets engines
with the `vault` backend ([kv-v2](https://www.vaultproject.io/docs/secrets/kv/kv-v2),
[pki](https://www.vaultproject.io/docs/secrets/pki), [databases](https://www.vaultproject.io/docs/secrets/databases),
[transit](https://www.vaultproject.io/docs/secrets/transit)) which require using 2 differents http methods (`GET`
by default, and `POST`).

Variables are substitued in secrets' keys before beeing processed by rconfd. Here, `${NAMESPACE}` allows you to
scope the vault role to the namespace where you have deployed your pod.

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
		"cmd": "echo reloading"
	}
}
```

The root keys of the config files are jsonnet templates (absolute or relative to `-d` argument). Each template is
a multi file output jsonnet template, meaning that its root keys represent the paths of the files to be generated
(absolute or relative to `dir`), while the values represent the files' content. `user` and `mode` set the owner
and file permissions on successful manifestation.

`secrets` maps secret path to variable name, and are accessible inside jsonnet templates through a
`secrets` [extVar](https://jsonnet.org/ref/stdlib.html) variable. The path has the following syntax:
`backend:arg1,arg2,k1=v1,k2=v2:path`, and can contain environment variables expressions `${NAME}`, in which case
it is the resulting string, after substitutions, that should conform to the aforementioned syntax.

There are currently 3 supported back-ends:
- `vault`: fetch a secret from the vault server using `arg1` as a `role` name for authentication, and `arg2` as the
   optional http method (`GET` by default). keywords arguments are sent as json dictionary in the body of the request.
- `env`: fetch the environment variable and parse it as a json if `arg1` == `js` or keep it as a string if `str`
- `file`: fetch the content of the file and parse it as a json value if `arg1` == `js` or keep it as a string if `str`

The secrets are collected among all templates and all config files (to fetch each secret only once) and the `cmd`
is executed if any of the config file change after manifestation.

# S6 integration

As rconfd has been made to configure (and actively reconfigure) one or several services configurations files,
you need at least 2 services in your container. [s6](https://skarnet.org/software/s6/) supervision suite is a
natural fit for managing multi services containers. It's simple as in clever, and extremely lightweight (full suite
under 900K in alpine). [s6-overlay](https://github.com/just-containers/s6-overlay) can kickstart you in minutes for
using it inside your containers.

One key component of s6 is [execline](https://skarnet.org/software/execline/) which aim is to replace your interpreter
(ie. bash) with a no-interpreter. An execline script is in fact one command line where each command consumes its
own arguments, complete its task and then replaces itself with the remaining arguments (chainloading), leaving
no trace of its passage after that. The script is parsed only once at startup and no interpreter lies in memory
during the process, and yet you can do everything a bash can do. It looks like an *impossible mission* script that
is consuming itself to the end, as only the remaining script stays in memory at each step. No interpreter means
fewer security risks (no injection possible with execline), fewer resources allocated, and instant startup.

This is the `/etc/services.d/rconfd/run` script I use in my s6-overlay + rconfd based image. In the service
directory you can put a `/etc/services.d/rconfd/notification-fd` with the content `3` which indicates that you want
[s6-supervise](https://skarnet.org/software/s6/s6-supervise.html) to open a service readiness file descriptor
on fd 3 (0: stdin, 1: stdout, 2: stderr). You can also create a `/etc/services.d/rconfd/timeout-finish` with a
`11000` value to delay the restart of rconfd service to 11s in case of error (template, vault access, io error, ...)

```sh
#!/usr/bin/execlineb -P
with-contenv
importas -u -D https://vault:8200/v1 VAULT_URL VAULT_URL
foreground { /usr/bin/rconfd -D -u ${VAULT_URL} -d /etc/rconfd -j /etc/rconfd -r 3 }
importas -u ? ?
if { s6-test ${?} = 0 }
	s6-pause
```

- [`with-contenv`](https://github.com/just-containers/s6-overlay/blob/master/builder/overlay-rootfs/usr/bin/with-contenv)
  allows to import container enviroment in the script (which can define `VAULT_URL`).
- [`importas`](https://skarnet.org/software/execline/importas.html) substitutes variables expressions present in
  its args (remaining script) using default value (`-D`) if undefined.
- then we launch rconfd in daemon mode, reading all config files in `/etc/rconfd` directory, using the readiness
  fd 3, and waiting for its completion in the foreground
- if the daemon exits normally (because only static secrets are used and it's useless to stay running in this
  case), we replace rconfd with the smallest daemon implementation possible
  ([`s6-pause`](https://skarnet.org/software/s6-portable-utils/s6-pause.html)), which just wait forever without
  consuming any resources (but still react to restart signals). Otherwise, rconfd service will just be
  restarted by s6-supervise. It is important that s6 considers the rconfd service always runnning, otherwise dependent
  services could wait indefinitely for rconfd readiness signal, thus the use of `s6-pause`.

For other services you then use startup script like this one, to passively wait until rconfd generate all config files

```sh
#!/usr/bin/execlineb -P
foreground { s6-svwait -U /var/run/s6/services/rconfd }
importas -u ? ?
if { s6-test ${?} = 0 }
	foreground { s6-echo start myservice }
	s6-setuidgid myservice
	cd /var/lib/myservice
	/usr/bin/myservice
```

In the `cmd` part of the rconfd config file you can use [`s6-svc`](https://skarnet.org/software/s6/s6-svc.html)
to signal (here a simple reload) a given service that configuration have changed

```json
{
        "cmd": "/bin/s6-svc -h /var/run/s6/services/myservice"
}
```

# Example

You should correctly
- [setup a vault server](https://learn.hashicorp.com/tutorials/vault/kubernetes-raft-deployment-guide?in=vault/kubernetes)
- [activate one or several secret engines](https://www.vaultproject.io/docs/secrets),
- [activate kubernetes auth method](https://www.vaultproject.io/docs/auth/kubernetes),
- [create policies](https://www.vaultproject.io/docs/concepts/policies) allowing your roles to access the secrets
  inside the back-ends
- create some secrets

Using the rconfd config file `test.json` above, we could write the following `test.jsonnet` template to create a json
file: `dump.json` (relative to `/etc/test`) and 3 text files: `/etc/ssl/cert.crt`, `/etc/ssl/cert.key`, and `test.txt`
(relative to `/etc/test`). `test.txt` file is only generated if the `file.json` indicated in the rconfd config
file and imported in the `secrets['file']` variable, has a root key `test` with a `true` value.

```jsonnet
local secrets = std.extVar("secrets");
{
	// we define shortcuts for easy access to the secret extVar content
	// the :: is to hide the corresponding key in the final result, avoiding generating a file with the same name
	// kv2 secret backend contains data and metadata so go directly to the data
	mysecret:: secrets['mysecret']['data'],
	mysecret2:: secrets['mysecret2'],
	namespace:: secrets['namespace'],
	file:: secrets['file'],
	cert:: secrets['cert'],

	// just dump all secrets using json manifestation
	'dump.json': std.manifestJsonEx({
		mysecret: $.mysecret,
		// remove the hmac prefix
		mysecret2: std.split($.mysecret2.hmac, ':')[2],
		namespace: $.namespace,
		file: $.file,
		cert: $.cert
	}, '  ')

	// save certificate and key in separate files
	'/etc/ssl/cert.crt': $.cert['certificate'],
	'/etc/ssl/cert.key': $.cert['private_key'],

	// conditional file manifestation
	[if secrets['file']['test'] == 'true' then 'test.txt']: 'hello world!'
}
```

# FAQ

## Why rconfd is exiting with no error code in daemon mode ?

rconfd in daemon mode can exist with no error code, leaving only the message `Exiting daemon mode: no dynamic
secrets used`. Without secrets to renew, rconfd considers that it's useless to wait for nothing and delegates
the task to keep running without doing anything to something else (lighter). It's a feature actually, as explained in
[s6 Integration](#s6-integration) section above.
