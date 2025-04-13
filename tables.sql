CREATE TABLE IF NOT EXISTS invitations (
    id SERIAL PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    unix BIGINT NOT NULL,
    msg_id VARCHAR(255) NOT NULL,
    invite TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS rsvps (
    id SERIAL PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    invitation_id INT NOT NULL,
    unix BIGINT NOT NULL,
    FOREIGN KEY (invitation_id) REFERENCES invitations(id)
);
