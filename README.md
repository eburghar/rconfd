# rconfd

`rconfd` is a lightweight utility for containers and CI/CD, written in async rust, to generate config files
from [jsonnet templates](https://jsonnet.org/), and eventually keep them in sync with secrets fetched from a
[vault server](https://www.vaultproject.io/) using a JWT token to authenticate with. Depending on the context,
you can use [JWT/OIDC Auth Method](https://www.vaultproject.io/docs/auth/jwt) for CI/CD or [Kubernetes Auth
Method](https://www.vaultproject.io/docs/auth/kubernetes) for service inside containers.

It can use the simple and yet effective [startup notification](https://skarnet.org/software/s6/notifywhenup.html)
mechanism of the [s6 supervision suite](https://skarnet.org/software/s6/) to signal other services that their
configuration files have been generated and it can launch arbitrary command when ready or configuration changed.

`rconfd` is a rewrite of the C++ [cconfd](https://github.com/eburghar/cconfd) utility
using the [blazing fast](https://github.com/CertainLach/jrsonnet#Benchmarks) [jrsonnet
interpreter](https://github.com/CertainLach/jrsonnet). `cconfd`, while working, was a failed attempt using stdc++17
and the [google/fruit](https://github.com/google/fruit) dependency injection library. It was way too hard to
understand and maintain. I never managed to allocate resources to add the missing features or fix some obvious
bugs. In contrast I ported all `cconfd` features in `rconfd` in just 2 days, and now as I consider it feature
complete, I know it is also faster, smarter, lighter, maintainable, thread and memory safe.

# Yet another configuration template manager ?

There is a lot of alternatives for generating configuration files at runtime under kubernetes using
various template engines and secrets back-ends ([confd](https://github.com/kelseyhightower/confd),
[consul-template](https://github.com/hashicorp/consul-template)...) but because such a tool can run in a lot
of containers inside the same host, I wanted the lightest and fastest implementation as possible with a minimal
surface attack, even at the cost of some flexibility (few back-ends, one template engine). Having this tool written
in Rust gives you safeness, correctness and easy maintenance with no special efforts while matching C speed.

For CI/CD, people traditionally use tools that expose secrets to enviroment variables (like
[envconsul](https://github.com/hashicorp/envconsul)). [`envlt`](https://github.com/eburghar/envlt.git) is a
lightweight alternative (or companion) of `rconfd` that does just that without embeding a jsonnet interpreter. Because
secrets can be structured and jsonnet allow to destructure them without the need of external tools, `rconfd`
can be preferable for complex CI/CD cases over `envlt`.

# jsonnet ?

Configuration files are structured by nature. Using a text templating system ([mustache](https://mustache.github.io/)
like) for generating them expose you to malformations (you forgot to close a `(` or a `{`, or introduced a bad indent
in a loop, ...), injection attacks, and escaping hell. With jsonnet it's impossible to generate malformed files,
unless you use string templates, which defeat the purpose of using jsonnet (objects) in the first place.

[jsonnet](https://jsonnet.org/) permits using complex operations for merging, adding, overriding and allows you
to easily and securely specialize your configuration files. By using mounts or environment variables in your
kubernetes manifests, along with the `file` and `env` back-ends, you can easily compose your configuration files
at startup in a flexible way.

# Process supervisor inside containers ?

If you have short-lived secrets tied to a service running in a container, you can run rconfd (not in daemon mode)
before your service, expect your service to fail after a while (ex: database credentials expired) and entrust
kubernetes to restart your pod quickly (with updated credentials).

Like the [S6 overlay authors](https://github.com/just-containers/s6-overlay#the-docker-way), I never believed
in the rigid general approach of one executable per container, which forces you to decouple your software stack
under kubernetes into pods, init containers, inject containers, side car containers, with liveliness and readiness
tests and blind kill and restart on timeout if conditions are not not met (which is the approach taken by [vault
injector](https://learn.hashicorp.com/tutorials/vault/kubernetes-sidecar?in=vault/kubernetes)).

With several services and `rconfd` in the same container supervised by s6, everything stays coherent and tied
together. The orchestration is simple and smarter, it starts faster, and scale without putting unnecessary pressure
on supervisor or container runtime.

# Setup

- deploy a [vault server](https://learn.hashicorp.com/tutorials/vault/kubernetes-raft-deployment-guide?in=vault/kubernetes)
- activate [one or several secret engines](https://www.vaultproject.io/docs/secrets),
- activate [kubernetes auth method](https://www.vaultproject.io/docs/auth/kubernetes) and [jwt auth
  method](https://www.vaultproject.io/docs/auth/jwt),
- create [policies](https://www.vaultproject.io/docs/concepts/policies) allowing your roles to access the secrets
  inside the back-ends
- create some secrets

# Usage

```
rconfd 0.11.1

Usage: rconfd [-d <dir>] [-u <url>] [-l <login-path>] [-j <jpath>] [-c <cacert>] [-T <token>] [-t <token-path>] [-v] [-r <ready-fd>] [-D]

Generate files from jsonnet templates and eventually keep them in sync with secrets fetched from a vault server using a jwt token to authenticate with.

Options:
  -d, --dir         directory containing the rconfd config files (/etc/rconfd)
  -u, --url         the vault url ($VAULT_URL or https://localhost:8200/v1)
  -l, --login-path  the login path (/auth/kubernetes/login)
  -j, --jpath       , separated list of aditional path for jsonnet libraries
  -c, --cacert      path of vault CA certificate
                    (/var/run/secrets/kubernetes.io/serviceaccount/ca.crt)
  -T, --token       the JWT token taken from the given variable name or from the
                    given string if it fails (take precedence over -t)
  -t, --token-path  path of the JWT token
                    (/var/run/secrets/kubernetes.io/serviceaccount/token)
  -v, --verbose     verbose mode
  -r, --ready-fd    s6 readiness file descriptor
  -D, --daemon      daemon mode (stays in the foreground)
  --help            display usage information

```

`rconfd` takes its instructions from one or several json files laying inside a directory (`-d` argument).

Each configuration file declares one or several jsonnet template files which in turn generate one or several
files.

Here is a simple `test.json` file declaring only one template, and using 4 different
secrets backends (`vault`, `env`, `file` and `exe`). We also use 4 different secrets
engines with the `vault` backend ([kv-v2](https://www.vaultproject.io/docs/secrets/kv/kv-v2),
[pki](https://www.vaultproject.io/docs/secrets/pki), [databases](https://www.vaultproject.io/docs/secrets/databases),
[transit](https://www.vaultproject.io/docs/secrets/transit)) which require using 2 differents http methods (`GET`
by default, and `POST`).

Variables are substitued in secrets' keys and `dir` value, before beeing processed by rconfd. Here, `${NAMESPACE}`
allows you to scope the vault role to the namespace where you have deployed your pod, while `${INSTANCE}` allows you
to change the final destination of relative manifests at runtime (you can't use variables in jsonnet keys).

```json
{
	"test.jsonnet": {
		"dir": "/etc/test/${INSTANCE}",
		"mode": "0644",
		"user": "test-user",
		"secrets": {
			"vault:${NAMESPACE}-role:kv/data/test/mysecret": "mysecret",
			"vault:${NAMESPACE}-role:database/creds/mydb": "mydb",
			"vault:${NAMESPACE}-role,POST,common_name=example.com:pki/issue/example.com": "cert",
			"vault:${NAMESPACE}-role,POST,input=password:transit/hmac/mysecret": "mysecret2",
			"env:str:NAMESPACE": "namespace",
			"file:js:file.json": "file",
			"exe:str:/usr/bin/nproc --all": "cpu",
			"exe:str,dynamic:/usr/bin/date +%s": "timestamp"
		},
		"hooks": {
			"modified": "/usr/bin/echo reloading",
			"ready": "/usr/bin/echo all files generated"
		}
	}
}
```

The root keys of the config files are jsonnet templates path (absolute or relative to `-d` argument). Each template is
a multi file output jsonnet template, meaning that its root keys represent the paths of the files to be generated
(absolute or relative to `dir`), while the values represent the files' content. `user` and `mode` set the owner
and file permissions on successful manifestation if rconfd is executed as root.

`secrets` maps a secret path to a variable name which become accessible inside jsonnet templates through a
`secrets` [extVar](https://jsonnet.org/ref/stdlib.html) object variable.

# Path expression

A path has the following syntax: `backend:args:path`.

It can contain environment variables expressions (`${NAME}`), in which case it is the resulting string, after
substitutions, that should conform to the aforementioned syntax.

There are currently 4 supported back-ends. The secrets are collected among all templates and all config files (to
fetch each secret only once) and the `hooks.modified` is executed if any of the config file change after manifestation.

## Vault backend

`vault` backend is used to fetch a secret from the vault server. The general syntax is

```
vault:role[,GET|PUT|POST|LIST][,key=val]*:path
```

- `role` is the role name used for vault authentication,
- an optional http method that defaults to `GET`,
- optional keywords arguments that are sent as json dictionary in the body of the request,
- a path corresponding to the vault api point (without `/v1/`),

## Env backend

`env` backend is used to get a value from an environment variable. The general syntax is

```
env:str|js:name
```

the value is parsed as json if `js` or kept as is if `str`

## File backend

`file` backend is used to fetch a secret from the content of the file. The general syntax is

```
file:str|js:name
```

the value is parsed as json if `js` or kept as is if `str`

## Exe backend

`exe` backend is used to generate a secret from a command. The general syntax is

```
exe:str|js[,dynamic|static]:cmd args
```

- `cmd` must be absolute and start with `/`. It is executed with rconfd user or `nobody` (via sudo) if root,
- the trimmed output of `cmd` is parsed as json if `js` or kept as is if `str`
- if `dynamic`, the command is executed at each template manifestation, otherwise if omited or `static` it is
  executed only once at startup.


# jsonnet template

Using the rconfd config file `test.json` above, we could write the following `test.jsonnet` template to create:

- a json file `dump.json` (relative to `/etc/test`)
- 2 text files: `/etc/ssl/cert.crt`, `/etc/ssl/cert.key`,
- and one conditional text file `test.txt` (relative to `/etc/test`) which is only generated if the `file.json`
  (imported as json in the `secrets['file']` variable), has a root key `test` with a `true` value.

```jsonnet
local secrets = std.extVar("secrets");
{
	// we define shortcuts for easy access to the secret extVar content
	// the :: is to hide the corresponding key in the final result, avoiding generating a file with the same name
	// kv2 secret backend contains data and metadata so go directly to the data
	mysecret:: secrets['mysecret']['data'],
	// remove the hmac prefix
	mysecret2:: std.split(secrets['mysecret2'].hmac, ':')[2],
	namespace:: secrets['namespace'],
	file:: secrets['file'],
	cert:: secrets['cert'],
	// turn cpu into an int for calculation
	cpu:: std.parseInt(secrets['cpu']),
	timestamp:: secrets['timestamp'],

	// just dump all secrets using json manifestation
	'dump.json': std.manifestJsonEx({
		mysecret: $.mysecret,
		mysecret2: $.mysecret2,
		namespace: $.namespace,
		file: $.file,
		cert: $.cert,
		cpu: $.cpu,
		timestamp: $.timestamp
	}, '  ')

	// save certificate and key in separate files
	'/etc/ssl/cert.crt': $.cert['certificate'],
	'/etc/ssl/cert.key': $.cert['private_key'],

	// conditional file manifestation
	[if secrets['file']['test'] == 'true' then 'test.txt']: 'hello world!'
}
```

# S6 integration

As rconfd has been made to configure (and actively reconfigure) one or several services configurations files,
you need at least 2 services running in your container. [s6](https://skarnet.org/software/s6/) supervision suite is a
natural fit for managing multi services containers. It's simple as in clever, and extremely lightweight (full suite
under 900K in alpine). [s6-overlay](https://github.com/just-containers/s6-overlay) can kickstart you for
using it inside your containers.

One key component of s6 is [execline](https://skarnet.org/software/execline/) which aim is to replace your
interpreter (ie. bash) with a no-interpreter. An execline script is in fact a chain of commands + arguments. Each
command consumes its own arguments, complete its task and then replaces itself with the remaining arguments
(chainloading). The script is parsed only once at startup and no interpreter lies in memory during the process,
and yet you can do everything a bash can do. It looks like an *impossible mission* script that is consuming itself
to the end. Only the remaining script stays in memory at each step. No interpreter means fewer security risks
(no injection possible with execline), fewer resources allocated, and instant startup.

This is the `/etc/services.d/rconfd/run` script I use in my s6-overlay + rconfd based image. In the service
directory you can put a `/etc/services.d/rconfd/notification-fd` with the content `3` which indicates that you want
[s6-supervise](https://skarnet.org/software/s6/s6-supervise.html) to open a service readiness file descriptor
on fd 3 (0: stdin, 1: stdout, 2: stderr).

```sh
#!/usr/bin/execlineb -P
with-contenv
foreground { /usr/bin/rconfd -D -j /etc/rconfd -r 3 }
importas -u ? ?
if { s6-test ${?} = 0 }
	s6-pause
```

- [`with-contenv`](https://github.com/just-containers/s6-overlay/blob/master/builder/overlay-rootfs/usr/bin/with-contenv)
  allows to import container enviroment (which can define `VAULT_URL`) in the script context.
- [`importas`](https://skarnet.org/software/execline/importas.html) substitutes variables expressions present in
  its args (remaining script) using default value (`-D`) if undefined.
- we then launch rconfd in daemon mode, reading all config files in `/etc/rconfd` directory, using the readiness
  fd 3, and waiting for its completion in the foreground
- if the daemon exits normally (because no leased secrets are used and it's useless to stay running in this
  case), we replace rconfd with the smallest daemon implementation possible
  ([`s6-pause`](https://skarnet.org/software/s6-portable-utils/s6-pause.html)), which just wait forever without
  consuming any resources (but still react to restart signals). Otherwise, rconfd service will just be
  restarted by s6-supervise. It is important that s6 considers the rconfd service always runnning, otherwise dependent
  services could wait indefinitely for rconfd readiness signal (thus the use of `s6-pause`).

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

In the `hooks.modified` part of the rconfd config file you can use [`s6-svc`](https://skarnet.org/software/s6/s6-svc.html)
to signal (here a simple reload with `SIGHUP`) a given service that configuration have changed

```json
{
		"hooks": {
			"modified": "/bin/s6-svc -h /var/run/s6/services/myservice"
		}
}
```

# Using rconfd with Gitlab CI/CD

## Configuring vault

Activate vault jwt authentication

```sh
vault write auth/jwt/config jwks_url="https://gitlab.com/-/jwks" bound_issuer="gitlab.com"
```

Create a policy for accessing the secrets

```sh
vault policy write mypolicy - <<EOF
path "kv/data/secrets/*" {
  capabilities = [ "read" ]
}
EOF
```

Create a role. Here You can only login with that role if the project is inside the `alpine` group and the
build is upon a protected tag (generally used for release).

```sh
vault write auth/jwt/role/myrole - <<EOF
{
  "role_type": "jwt",
  "policies": ["mypolicy"],
  "token_explicit_max_ttl": 60,
  "user_claim": "user_email",
  "bound_claims": {
    "group_path": "alpine",
    "ref_protected": "true",
    "ref_type": "tag"
  }
}
```

## Configuring CI/CD

You should make a build image (`mybuilder`) containing the rconfd configuration files (`/etc/rconfd`) and rconfd
executable. Then you just have to call rconfd in your pipelines script reading the JWT token from the environment
variable `CI_JOB_JWT` (note that we use a variable name here and and not a substitution to not expose the token on
the command line arguments), and redefine the login path to `/auth/jwt/login` before calling your build script. You
must define a `VAULT_URL` variable, and a good place for that is in project or group settings.

Here is an example `.gitlab-ci.yml`

```yaml
image: mybuilder

before_script:
  # generate all needed configuration files for Makefile
  - rconfd -T CI_JOB_JWT -l /auth/jwt/login

build:
  stage: build
  script:
  # The Makefile use files containing secrets generated by rconfd
  - make
```

# FAQ

## Why rconfd is exiting with no error code in daemon mode ?

rconfd in daemon mode can exit with no error code, leaving only the message `Exiting daemon mode: no leased
secrets used`. Without secrets to renew, rconfd considers that it's useless to wait for nothing and delegates
the task to keep running without doing anything to something else (lighter). It's a feature actually, as explained in
[s6 Integration](#s6-integration) section above.
