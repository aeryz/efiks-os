import gdb

class WalkSv39(gdb.Command):
    """walksv39 ROOT_PT_PA VA DIRECT_BASE
Walk Sv39 page table using DIRECT_BASE + PA for memory reads."""

    def __init__(self):
        super(WalkSv39, self).__init__("walksv39", gdb.COMMAND_USER)

    def invoke(self, arg, from_tty):
        args = gdb.string_to_argv(arg)
        if len(args) != 3:
            print("usage: walksv39 ROOT_PT_PA VA DIRECT_BASE")
            print("example: walksv39 0x80227000 0x11170 0xffffffd600000000")
            return

        root_pa = int(gdb.parse_and_eval(args[0]))
        va = int(gdb.parse_and_eval(args[1]))
        direct_base = int(gdb.parse_and_eval(args[2]))

        vpn0 = (va >> 12) & 0x1ff
        vpn1 = (va >> 21) & 0x1ff
        vpn2 = (va >> 30) & 0x1ff
        off  = va & 0xfff

        print(f"VA: 0x{va:x}")
        print(f"ROOT PT PA: 0x{root_pa:x}")
        print(f"DIRECT_BASE: 0x{direct_base:x}")
        print(f"VPN2={vpn2:#x} VPN1={vpn1:#x} VPN0={vpn0:#x} off={off:#x}")

        def pa_to_kva(pa):
            return direct_base + pa

        def read_u64_pa(pa):
            kva = pa_to_kva(pa)
            return int(gdb.parse_and_eval(f"*(unsigned long*)0x{kva:x}"))

        def ppn_to_pa(pte):
            return ((pte >> 10) << 12)

        def flags(pte):
            names = ["V", "R", "W", "X", "U", "G", "A", "D"]
            return "|".join(n for i, n in enumerate(names) if pte & (1 << i)) or "-"

        def is_leaf(pte):
            return (pte & 0b1110) != 0  # R/W/X

        l2_pa = root_pa + vpn2 * 8
        pte2 = read_u64_pa(l2_pa)
        print(f"L2 PA 0x{l2_pa:x} KVA 0x{pa_to_kva(l2_pa):x}: PTE=0x{pte2:016x} flags={flags(pte2)}")

        if not (pte2 & 1):
            print("Invalid L2 PTE")
            return
        if is_leaf(pte2):
            print("L2 is leaf: 1 GiB mapping")
            print(f"PA = 0x{ppn_to_pa(pte2) + (va & 0x3fffffff):x}")
            return

        pt1_pa = ppn_to_pa(pte2)
        print(f" -> L1 table PA: 0x{pt1_pa:x} KVA: 0x{pa_to_kva(pt1_pa):x}")

        l1_pa = pt1_pa + vpn1 * 8
        pte1 = read_u64_pa(l1_pa)
        print(f"L1 PA 0x{l1_pa:x} KVA 0x{pa_to_kva(l1_pa):x}: PTE=0x{pte1:016x} flags={flags(pte1)}")

        if not (pte1 & 1):
            print("Invalid L1 PTE")
            return
        if is_leaf(pte1):
            print("L1 is leaf: 2 MiB mapping")
            print(f"PA = 0x{ppn_to_pa(pte1) + (va & 0x1fffff):x}")
            return

        pt0_pa = ppn_to_pa(pte1)
        print(f" -> L0 table PA: 0x{pt0_pa:x} KVA: 0x{pa_to_kva(pt0_pa):x}")

        l0_pa = pt0_pa + vpn0 * 8
        pte0 = read_u64_pa(l0_pa)
        print(f"L0 PA 0x{l0_pa:x} KVA 0x{pa_to_kva(l0_pa):x}: PTE=0x{pte0:016x} flags={flags(pte0)}")

        if not (pte0 & 1):
            print("Invalid L0 PTE")
            return
        if not is_leaf(pte0):
            print("L0 is valid but non-leaf: malformed for Sv39")
            return

        pa_page = ppn_to_pa(pte0)
        pa = pa_page + off

        print("4 KiB leaf mapping:")
        print(f"  page PA  = 0x{pa_page:x}")
        print(f"  final PA = 0x{pa:x}")
        print(f"  final KVA via direct map = 0x{pa_to_kva(pa):x}")

WalkSv39()
