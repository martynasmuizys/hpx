/// Base template
pub static BASE_IP: &str = "// clang-format off
#include \"vmlinux.h\"
#include <bpf/bpf_helpers.h>
// clang-format on

struct Data {
    __u32 ip;
	__u64 rx_packets;
    __u64 last_access_ns;
};

static __u64 MS_IN_NS = 100000;

{{whitelist_map}}

{{blacklist_map}}

{{graylist_map}}

SEC(\"xdp\")
int {{name}}(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    // Check Ethernet header size
    if (data + sizeof(struct ethhdr) > data_end)
        return XDP_PASS;

    struct ethhdr *eth = data;

    // Check IP header size
    struct iphdr *ip = data + sizeof(struct ethhdr);
    if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) > data_end)
        return XDP_PASS;

    if(ip->protocol != IPPROTO_TCP) {
        return XDP_PASS;
    }

    // Extract source IP address
    __u32 src_ip = ip->saddr;

    {{whitelist_action}}

    {{blacklist_action}}

    {{graylist_action}}

    return XDP_PASS;
}


char __license[] SEC(\"license\") = \"GPL\";
";

pub static BASE_DNS: &str = "// clang-format off
#include \"vmlinux.h\"
#include <bpf/bpf_helpers.h>
// clang-format on

#define htons(x) __builtin_bswap16(x)

struct Data {
  __u32 ip;
  __u64 rx_packets;
  __u64 last_access_ns;
};

{{whitelist_map}}
{{blacklist_map}}

SEC(\"xdp\")
int {{name}}(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    // Check Ethernet header size
    if (data + sizeof(struct ethhdr) > data_end)
      return XDP_PASS;

    struct ethhdr *eth = data;

    // Check IP header size
    struct iphdr *ip = data + sizeof(struct ethhdr);
    if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) > data_end)
      return XDP_PASS;

    if (ip->protocol != IPPROTO_UDP) {
      return XDP_PASS;
    }

    struct udphdr *udp = (data + sizeof(struct ethhdr) + sizeof(struct iphdr));
    if ((void *)(udp + 1) > data_end) {
      return XDP_PASS; // Bounds check for UDP header
    }

    if (udp->dest != htons(53) && udp->source != htons(53)) {
      return XDP_PASS;
    }
    // Extract destination IP address
    __u32 dest_ip = ip->daddr;

    {{whitelist_action}}
    {{blacklist_action}}


  return XDP_PASS;
}

char __license[] SEC(\"license\") = \"GPL\";
";

/// Map template
pub static MAP: &str = "struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__type(key, __u32);
	__type(value, struct Data);
	__uint(max_entries, {{max}});
} {{name}} SEC(\".maps\");
";

/// Get ip/dns data
pub static GET_DATA_IP: &str = "struct Data *{{list}}_data = bpf_map_lookup_elem(&{{list}}, &src_ip);";
pub static GET_DATA_DNS: &str = "struct Data *{{list}}_data = bpf_map_lookup_elem(&{{list}}, &dest_ip);";

/// Allow/Deny action (for whitelist/blacklist)
pub static ACTION: &str = "if ({{list}}_data) {
		return {{action}};
	}
";

/// Investigate action (for graylist)
pub static GRAYLIST: &str = "if ({{list}}_data) {
    __u64 time = (__u64){{frequency}} * MS_IN_NS;
    if (bpf_ktime_get_ns() - {{list}}_data->last_access_ns < time) {
        struct Data new = {src_ip, {{list}}_data->rx_packets, bpf_ktime_get_ns()};
        bpf_map_update_elem(&blacklist, &src_ip, &new, BPF_NOEXIST);
        return XDP_DROP;
    }
    __sync_fetch_and_add(&{{list}}_data->rx_packets, 1);
    __sync_fetch_and_add(&{{list}}_data->last_access_ns, bpf_ktime_get_ns() - {{list}}_data->last_access_ns);
} else {
    struct Data new = {src_ip, 1, bpf_ktime_get_ns()};
    bpf_map_update_elem(&{{list}}, &src_ip, &new, BPF_NOEXIST);
}
";
