![cdlogo](https://carefuldata.com/images/cdlogo.png)

# kiagateway

Kiagateway is a minimalistic and high performance TCP domain-based traffic routing gateway.

Kiagateway is configured with a single TOML file that specifies the domain names and the backend IP
that receives traffic for that domain (SNI/Host).

The traffic is intercepted on 80 and 443 and routed to any configured backend matching the Host header (HTTP)
or the SNI value (HTTPS/TLS).

The TLS traffic is not decrypted, just the SNI is extracted and used to route the traffic to any backend matching
that SNI value (example.com).

The backends may likely be load balancer virtual IPs. Also see [kiaproxy](https://github.com/jpegleg/kiaproxy) for an example backend to use.

The architecture of kiagateway and kiaproxy can be used to support effective and high performance distributed systems networking, without the need
for anything fancy. They are simple, light on system resource use, few dependencies, low cost, easy to understand, easy to build with, reliable, fast, and secure!

## Why use kiagateway


If we have a server but we need to host multiple websites, kiagateway is made for that.

If we hae a giant enterprise with many domains and need in-cluster microservice routing by domain, kiagateway is made for that.

If we want to create a WAN layer to send domain traffic to different load balancers, kiagateway is made for that.

There is a high degree of utility in kiagateway, as it can be used in so many places due to it's compact size and performance: on the WAN, on the LAN, on a firewall, on a load balancer, in a sidecare container, and more.
    

## Installation

Kiagateway is available on [github](https://github.com/jpegleg/kiagateway/), [crates.io](https://crates.io/crates/kiagateway), and [docker hub](https://hub.docker.com/r/carefuldata/kiagateway).

The container image is very small and hardened, with only a single statically linked Rust binary added to a minimized container "scratch" image.

Here is an example of pulling the image from docker hub and running via Podman or Docker:

```
podman pull docker.io/carefuldata/kiagateway:latest
podman run -d -it --network=host carefuldata/kiagateway -v /opt/kiagateway_live/servers.toml:/servers.toml

```

Kiagateway only listens on port 80 and 443, those are not configurable. If you need to change those
or make some variation to that, clone the repo and change the ports. But the intent is for kiagateway
to proxy ports 80 and 443 for standard web ingress.

Installing via Cargo:

```
cargo install kiagateway
```

Kiagateway can also be compiled from source or installed from precompiled release binaries via github.

Kiagateway works well in Kubernetes, too, just specify the TOML config in the manifest.

This is a simplistic manifest example, just to show the general concept. There are of course many more advanced or refined
manifest possibilities.

```
---
apiVersion: apps/v1
kind: ConfigMap
metadata:
  name: gatewaycfgz
  namespace: green
  annotations:
    app.kubernetes.io/instance: gatewaycfgz
    app.kubernetes.io/version: 0.0.1
data:
  servers.toml: |
[backends]
"example.com" = "127.0.0.1:8001"
"example.org = "127.0.0.1:8002"
...
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kiagateway
  labels:
    app.kubernetes.io/name: kiagateway
spec:
  replicas: 2
  selector:
    matchLabels:
      app: kiagateway
  template:
    metadata:
      labels:
        app: kiagateway
        app.kubernetes.io/name: kiagateway
    spec:
      containers:
      - name: kiagatewpodman volume mount a fileay
        image: "carefuldata/kiagateway:latest"
        ports:
        - name: tls-passthrough
          containerPort: 443
        - name: http-passthrough
          containerPort: 80
        volumeMounts:
        - name: gatewaycfgz
          mountPath: /
      volumes:
      - name: gatewaycfgz
        configMap:
          name: gatewaycfgz
...

```

You can create your own container image easily as well. This example shows building a new image with a different exposed port set to ports 80 through 5000 and
is assuming a musl statically linked binary is already in $PWD for the image build. Compile kiagateway on Alpine Linux, or extract the existing
one from the public container image, or download one from github, to get such a binary. The compile can obviously be added to the Dockefile
in an earlier step, or compiled in a dynamically linked way and used in an image with the right C libraries for your target.

```
FROM scratch
COPY ./kiagateway /kiagateway
EXPOSE 80-5000
CMD ["/kiagateway"]
```

## So, what about HA of kiagateway itself?

Kiagateway collapses ingress traffic, but in order for kiagateway itself to be highly available, we need a second kiagateway instance, ideally on separate physical hardware.

The cheap and easy way is to have two different computers running kiagateway (on separate hardware) and have DNS records for both, however that can still lead to outages.
A better solution is to use [GSLB](https://www.ibm.com/think/topics/global-server-load-balancing), and/or something like [CARP](https://www.openbsd.org/faq/pf/carp.html),
selecting which kiagateway server to use in order to minimize downtime for kiagateway itself.

<b>Important note for running kiagateway on OpenBSD:</b> The file limits (the important default to change is defined in /etc/login.conf) should be raised from the OpenBSD defaults, otherwise a DoS condition is possible.
The other limit on OpenBSD is from (sysctl kern.maxfiles), which is the global limit that is usually more reasonable. I would ensure kiagateway can get over 4,000 files, and wouldn't feel bad about going higher, especially
if internet facing.

Kiagateway might be used _in front_ of Kubernetes clusters, but can be run within Kubernetes clusters, or maybe in it's own dedicated "load balancer cluster", etc etc.


## Project promises

This project will never use AI-slop. All code is reviewed, tested, and implemented by a human expert. This repository and the crates.io repository are carefully managed and protected.

This project will be maintained as best as is reasonable.
