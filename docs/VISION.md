Key problems being solved:
The cloud sucks for agents to work on. They are much better users of local file systems and local FS manipulation
Other companies store your data. Lame. Insecure. Dangerous.
Private can never be collaborative in today’s day and age. Everything can be subpoenaed; anyone can leak anything.

Proposed solution:
The world’s first local Google Drive
Files are distributed across multiple ‘clients’ in the same ‘network’ (network here doesn’t mean an internet network but just a selection of clients to be participants in this file system protocol)
Similar to how early-day Napster worked (some clients would have songs preloaded; any nearby clients would first check with clients in their vicinity to see if they have the song they’re looking for, and only then would they otherwise query a server)
We need to come up with a paradigm to secure the absolute crap out of any files being stored on the clients’ computers such that even they (since the files are owned communally) can not tamper with the files. And if someone deletes a certain file on their end, no other client will ever be able to recover it. Is there some way we can rotate hashes? Or create a physical piece of hardware that users must plug into their computers to use this file system?
Generally, no server, ever. Maybe?
Powerful function to clear files when too much of your storage is being utilized, wherein it’ll check for each file which peers have access to it already, and selectively delete accordingly. We can also have this scheduled periodically for users OR when a certain percent of their computer’s storage is being overutilized.
Once a new file/folder is created, send out the ID of that folder to ALL connected clients, as well as where it exists. Once a client receives a file/folder on their end, once again poll ALL connected clients to tell them where the file/folder now exists, so that they know where to go when they actually have to download it, and also later on for when they have to clear up storage space on their computer.
Keys rotated weekly
An actual GUI (desktop/mobile/tablet app) that will reflect all of these changes happening in real-time. Super real-time. Like if I move a folder into another folder, it will show that my cursor as Aaryaman Maheshwari, just did that action, in real-time.
Encrypted packets via AES key (symmetric encryption), but we need to be able to exchange keys without anyone seeing what the keys are, so we use a post-quantum public key algorithm to share keys to each person individually. When it comes to public keys, everyone has their own unique private key, and they publish their public keys. Anybody can see the public keys; they can only encrypt info with the public key. For every pair of individuals, there is an AES key through which they communicate.
Files aren’t removable from the group unless there is auto-
Changes must automatically propagate - if user A updates and SAVES (i.e. pushes) a file, ALL chunks of this file across connected clients must update as such
Queue exists for each ‘offline member’ - the queue grows for all offline members
Essentially the queue is the same for every online member, in that updates are shared so if A makes change, then B makes change but C is offline, there is a queue from both A, B who send instructions
Full time server for queue requests

Problems to work around:
How do I access something no matter what, no matter when? I do not want to have to rely on the fact that someone else
If I’m sitting in India and the file I want is on a client in the US, is it possible for me to access it securely at all, ever, and in a way as efficient as using the cloud is?
If you kick someone from a shared folder, they still hold the old key and may hold old copies
How do we remove people from the file system and ensure they can not access the files anymore