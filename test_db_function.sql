-- Test if is_player_allowed function exists
SELECT proname, prosrc 
FROM pg_proc 
WHERE proname = 'is_player_allowed';

-- Test function directly
SELECT is_player_allowed(1, 1);
