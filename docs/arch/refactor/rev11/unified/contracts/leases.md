# Ephemeral lease contract

Leases are runtime files outside authority. Admission binds the node and holder, the exact canonical merge-base SHA/tree, the candidate start SHA/tree, the one checked-out worktree, the authority digest, and the exact conflict-domain path-root and symbol scope. The candidate ref may advance only as a descendant of the admitted start until `candidate-finalize` freezes its final SHA/tree and exact base delta. Expired leases are ignored; malformed leases fail the state query. Two live leases may not overlap a conflict domain. Capacity/resource class affects scheduling only and cannot create correctness edges.

`dispatch` creates an immutable digest-bound packet plus a dispatch receipt; printing an unrecorded packet is not dispatch. Acceptance requires that exact dispatch receipt and exactly one candidate-finalization receipt under the same lease. Runtime artifacts use atomic no-replace installation, so concurrent imports cannot overwrite an existing identity.
