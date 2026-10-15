#include "state.h"

void capture(Connection* connection, IStatus* status, FbError* error) noexcept {
    error->category = 1;
    connection->master->getUtilInterface()->formatStatus(error->message, sizeof(error->message), status);
    const ISC_STATUS* items = status->getErrors();
    connection->sqlstate(error->sqlstate, items);
    // Copy error/warning codes, never dangling pointers from a status vector.
    while (*items != isc_arg_end && error->code_count < 64) {
        const auto tag = *items++;
        if (tag == isc_arg_gds || tag == isc_arg_warning) {
            error->codes[error->code_count++] = *items;
            if (*items == isc_cancelled && !error->kind) error->kind = 1;
            if (*items == isc_cfg_stmt_timeout || *items == isc_att_stmt_timeout || *items == isc_req_stmt_timeout) error->kind = 2;
            if (*items == isc_nothing_to_cancel) error->kind = 3;
        }
        // All documented arguments carry one word except counted strings.
        items += tag == isc_arg_cstring ? 2 : 1;
    }
}
