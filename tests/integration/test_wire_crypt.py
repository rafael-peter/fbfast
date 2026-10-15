"""Exercise actual encrypted/unencrypted negotiation on disposable database aliases."""

import pytest


@pytest.mark.parametrize("server", ["required", "enabled", "disabled"])
@pytest.mark.parametrize("client", ["required", "enabled", "disabled", True, False])
def test_wire_encryption_policy_and_boolean_aliases(fb, config, server, client):
    policy = ("required" if client else "disabled") if isinstance(client, bool) else client
    incompatible = {server, policy} == {"required", "disabled"}
    settings = config(database=f"fbfast_crypt_{server}", role=None,
                      wire_crypt=client, pool_size=1, min_idle=0)
    if incompatible:
        with pytest.raises(fb.AuthenticationError, match="Incompatible wire encryption") as failure:
            with fb.Pool(settings):
                pass
        assert 335545064 in failure.value.native_codes
        assert failure.value.sqlstate == "28000"
        assert "wire_crypt" in str(failure.value)
        return
    with fb.Pool(settings) as pool:
        encrypted = pool.fetch_val(
            "SELECT MON$WIRE_ENCRYPTED FROM MON$ATTACHMENTS "
            "WHERE MON$ATTACHMENT_ID = CURRENT_CONNECTION"
        )
        assert encrypted is (server != "disabled" and policy != "disabled")
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
